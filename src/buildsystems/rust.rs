use crate::analyze::{AnalyzedError};
use crate::dependency::Dependency;
use crate::dependencies::CargoCrateDependency;
use crate::buildsystem::{BuildSystem, Error, DependencyCategory};
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize)]
struct Package {
    name: String,
}

#[derive(serde::Deserialize)]
#[serde(untagged)]
enum CrateDependency {
    Version(String),
    Details {
        version: String,
        optional: Option<bool>,
        features: Option<Vec<String>>,
        default_features: Option<bool>,
    }
}

impl CrateDependency {
    fn version(&self) -> &str {
        match self {
            Self::Version(v) => v,
            Self::Details { version, .. } => version,
        }
    }

    fn features(&self) -> Option<&[String]> {
        match self {
            Self::Version(_) => None,
            Self::Details { features, .. } => features.as_ref().map(|v| v.as_slice()),
        }
    }
}

#[derive(serde::Deserialize)]
struct CargoToml {
    package: Package,
    dependencies: Option<std::collections::HashMap<String, CrateDependency>>,
}


pub struct Cargo {
    path: PathBuf,
    local_crate: CargoToml,
}

impl Cargo {
    pub fn new(path: PathBuf) -> Self {
        let cargo_toml = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
        let local_crate: CargoToml = toml::from_str(&cargo_toml).unwrap();
        Self {
            path,
            local_crate,
        }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("Cargo.toml").exists() {
            log::debug!("Found Cargo.toml, assuming rust cargo package.");
            Some(Box::new(Cargo::new(path.to_path_buf())))
        } else {
            None
        }
    }

    fn install_declared_requirements(&self, session: &dyn crate::session::Session, fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>) -> Result<(), Error> {
        if let Some(fixers) = fixers {
            session.command(vec!["cargo", "fetch"]).run_fixing_problems(fixers).unwrap();
        } else {
            session.command(vec!["cargo", "fetch"]).run_detecting_problems()?;
        }
        Ok(())
    }
}

impl BuildSystem for Cargo {
    fn name(&self) -> &str {
        "cargo"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        todo!()
    }

    fn test(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        session.command(vec!["cargo", "test"]).run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        match session.command(vec!["cargo", "generate"]).run_detecting_problems() {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, ..}) if lines == ["error: no such subcommand: `generate`"] => {}
            Err(e) => return Err(e.into()),
        }
        session.command(vec!["cargo", "build"]).run_detecting_problems()?;
        Ok(())
    }

    fn clean(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        session.command(vec!["cargo", "clean"]).run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), Error> {
        let mut args = vec!["cargo".to_string(), "install".to_string(), "--path=.".to_string()];
        if let Some(prefix) = install_target.prefix.as_ref() {
            args.push(format!("-root={}", prefix.to_str().unwrap()));
        }
        session.command(args.iter().map(|x| x.as_str()).collect()).run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, Error> {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        for (name, details) in self.local_crate.dependencies.as_ref().unwrap_or(&std::collections::HashMap::new()) {
            ret.push((DependencyCategory::Build, Box::new(CargoCrateDependency {
                name: name.clone(),
                features: Some(details.features().unwrap_or(&[]).to_vec()),
                api_version: None
            })));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        todo!()
    }
}
