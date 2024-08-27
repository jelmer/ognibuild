use crate::installer::{Installer, InstallationScope};
use crate::session::Session;
use crate::buildsystem::{BuildSystem, Error};
use crate::dependency::Dependency;
use std::path::{Path,PathBuf};

pub struct Waf {
    path: PathBuf,
}

impl Waf {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn setup(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let binary_req = crate::dependencies::BinaryDependency::new("python3");
        if !binary_req.present(session) {
            installer.install(&binary_req, InstallationScope::Global)?;
        }
        Ok(())
    }

    pub fn probe(path: &std::path::Path) -> Option<Box<dyn BuildSystem>> {
        let path = path.join("waf");
        if path.exists() {
            log::debug!("Found waf, assuming waf package.");
            Some(Box::new(Self::new(path)))
        } else {
            None
        }
    }
}

impl BuildSystem for Waf {
    fn name(&self) -> &str {
        "waf"
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session, installer)?;
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(std::path::Path::new(".")));
        crate::analyze::run_detecting_problems(session, vec!["./waf", "dist"], None, false, None, None, None, None, None, None)?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer)?;
        crate::analyze::run_detecting_problems(session, vec!["./waf", "test"], None, false, None, None, None, None, None, None)?;
        Ok(())
    }

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer)?;
        match crate::analyze::run_detecting_problems(session, vec!["./waf", "build"], None, false, None, None, None, None, None, None) {
            Err(crate::analyze::AnalyzedError::Unidentified{ lines, ..}) if lines.contains(&"The project was not configured: run \"waf configure\" first!".to_string()) => {
                crate::analyze::run_detecting_problems(session, vec!["./waf", "configure"], None, false, None, None, None, None, None, None)?;
                crate::analyze::run_detecting_problems(session, vec!["./waf", "build"], None, false, None, None, None, None, None, None)
            }
            other => other
        }?;
        Ok(())
    }

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        todo!()
    }
}
