use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::CargoCrateDependency;
use crate::dependency::Dependency;
use std::path::{Path, PathBuf};

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct Package {
    name: String,
}

#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
#[allow(dead_code)]
enum CrateDependency {
    Version(String),
    Details {
        version: Option<String>,
        optional: Option<bool>,
        features: Option<Vec<String>>,
        workspace: Option<bool>,
        git: Option<String>,
        branch: Option<String>,
        #[serde(rename = "default-features")]
        default_features: Option<bool>,
    },
}

#[allow(dead_code)]
impl CrateDependency {
    fn version(&self) -> Option<&str> {
        match self {
            Self::Version(v) => Some(v.as_str()),
            Self::Details { version, .. } => version.as_deref(),
        }
    }

    fn features(&self) -> Option<&[String]> {
        match self {
            Self::Version(_) => None,
            Self::Details { features, .. } => features.as_ref().map(|v| v.as_slice()),
        }
    }
}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
pub struct CrateBinary {
    name: String,
    path: Option<PathBuf>,
    #[serde(rename = "required-features")]
    required_features: Option<Vec<String>>,
}

#[derive(serde::Deserialize, Debug)]
pub struct CrateLibrary {}

#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct CargoToml {
    package: Option<Package>,
    dependencies: Option<std::collections::HashMap<String, CrateDependency>>,
    bin: Option<Vec<CrateBinary>>,
    lib: Option<CrateLibrary>,
}

#[derive(Debug)]
pub struct Cargo {
    #[allow(dead_code)]
    path: PathBuf,
    local_crate: CargoToml,
}

impl Cargo {
    pub fn new(path: PathBuf) -> Self {
        let cargo_toml = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
        let local_crate: CargoToml = toml::from_str(&cargo_toml).unwrap();
        Self { path, local_crate }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("Cargo.toml").exists() {
            log::debug!("Found Cargo.toml, assuming rust cargo package.");
            Some(Box::new(Cargo::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

impl BuildSystem for Cargo {
    fn name(&self) -> &str {
        "cargo"
    }

    fn dist(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &std::path::Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        Err(Error::Unimplemented)
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        session
            .command(vec!["cargo", "test"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        match session
            .command(vec!["cargo", "generate"])
            .run_detecting_problems()
        {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines == ["error: no such subcommand: `generate`"] => {}
            Err(e) => return Err(e.into()),
        }
        session
            .command(vec!["cargo", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        session
            .command(vec!["cargo", "clean"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        let mut args = vec![
            "cargo".to_string(),
            "install".to_string(),
            "--path=.".to_string(),
        ];
        if let Some(prefix) = install_target.prefix.as_ref() {
            args.push(format!("-root={}", prefix.to_str().unwrap()));
        }
        session
            .command(args.iter().map(|x| x.as_str()).collect())
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(
            crate::buildsystem::DependencyCategory,
            Box<dyn crate::dependency::Dependency>,
        )>,
        Error,
    > {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        for (name, details) in self
            .local_crate
            .dependencies
            .as_ref()
            .unwrap_or(&std::collections::HashMap::new())
        {
            ret.push((
                DependencyCategory::Build,
                Box::new(CargoCrateDependency {
                    name: name.clone(),
                    features: Some(details.features().unwrap_or(&[]).to_vec()),
                    api_version: None,
                    minimum_version: None,
                }),
            ));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        let mut ret: Vec<Box<dyn crate::output::Output>> = vec![];
        if let Some(bins) = &self.local_crate.bin {
            for bin in bins {
                ret.push(Box::new(crate::output::BinaryOutput::new(&bin.name)));
            }
        }
        // TODO: library output
        Ok(ret)
    }

    fn install_declared_dependencies(
        &self,
        _categories: &[crate::buildsystem::DependencyCategory],
        scopes: &[crate::installer::InstallationScope],
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<(), Error> {
        if !scopes.contains(&crate::installer::InstallationScope::Vendor) {
            return Err(crate::installer::Error::UnsupportedScopes(scopes.to_vec()).into());
        }
        if let Some(fixers) = fixers {
            session
                .command(vec!["cargo", "fetch"])
                .run_fixing_problems::<_, Error>(fixers)
                .unwrap();
        } else {
            session
                .command(vec!["cargo", "fetch"])
                .run_detecting_problems()?;
        }
        Ok(())
    }
}
