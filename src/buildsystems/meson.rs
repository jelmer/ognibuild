use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::vague::VagueDependency;
use crate::dependency::Dependency;
use crate::dist_catcher::DistCatcher;
use crate::fix_build::BuildFixer;
use crate::installer::Error as InstallerError;
use crate::session::Session;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct Meson {
    #[allow(dead_code)]
    path: PathBuf,
}

impl Meson {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn setup(&self, session: &dyn Session) -> Result<(), Error> {
        if !session.exists(Path::new("build")) {
            session.mkdir(Path::new("build")).unwrap();
        }
        session
            .command(vec!["meson", "setup", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn introspect(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
        args: &[&str],
    ) -> Result<serde_json::Value, InstallerError> {
        let args = [&["meson", "introspect"], args, &["./meson.build"]].concat();
        let ret = if let Some(fixers) = fixers {
            session
                .command(args)
                .run_fixing_problems::<_, Error>(fixers)
                .unwrap()
        } else {
            session.command(args).run_detecting_problems()?
        };

        let text = ret.concat();

        Ok(serde_json::from_str(&text).unwrap())
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let path = path.join("meson.build");
        if path.exists() {
            log::debug!("Found meson.build, assuming meson package.");
            Some(Box::new(Self::new(&path)))
        } else {
            None
        }
    }
}

impl BuildSystem for Meson {
    fn name(&self) -> &str {
        "meson"
    }

    fn dist(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session)?;
        let dc = DistCatcher::new(vec![session.external_path(Path::new("build/meson-dist"))]);
        match session
            .command(vec!["ninja", "-C", "build", "dist"])
            .run_detecting_problems()
        {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"ninja: error: unknown target 'dist', did you mean 'dino'?".to_string(),
                ) =>
            {
                unimplemented!();
            }
            Err(e) => return Err(e.into()),
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        session
            .command(vec!["ninja", "-C", "build", "test"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        session
            .command(vec!["ninja", "-C", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        session
            .command(vec!["ninja", "-C", "build", "clean"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        self.setup(session)?;
        session
            .command(vec!["ninja", "-C", "build", "install"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn Dependency>)>, Error> {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = Vec::new();
        let resp = self.introspect(session, fixers, &["--scan-dependencies"])?;
        for entry in resp.as_array().unwrap() {
            let version = entry.get("version").and_then(|v| v.as_array()).unwrap();
            let mut minimum_version = None;
            if version.len() == 1 {
                if let Some(rest) = version[0].as_str().unwrap().strip_prefix(">=") {
                    minimum_version = Some(rest.to_string());
                }
            } else if version.len() > 1 {
                log::warn!("Unable to parse version constraints: {:?}", version);
            }
            // TODO(jelmer): Include entry['required']
            ret.push((
                DependencyCategory::Universal,
                Box::new(VagueDependency {
                    name: entry["name"].as_str().unwrap().to_string(),
                    minimum_version,
                }),
            ));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        let mut ret: Vec<Box<dyn crate::output::Output>> = Vec::new();
        let resp = self.introspect(session, fixers, &["--targets"])?;
        for entry in resp.as_array().unwrap() {
            let entry = entry.as_object().unwrap();
            if !entry.get("installed").unwrap().as_bool().unwrap() {
                continue;
            }
            if entry.get("type").unwrap().as_str() == Some("executable") {
                for name in entry.get("filename").unwrap().as_array().unwrap() {
                    let p = PathBuf::from(name.as_str().unwrap());
                    ret.push(Box::new(crate::output::BinaryOutput::new(
                        p.file_name().unwrap().to_str().unwrap(),
                    )));
                }
            }
            // TODO(jelmer): Handle other types
        }

        Ok(ret)
    }
}
