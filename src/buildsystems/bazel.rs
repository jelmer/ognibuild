use std::path::{Path, PathBuf};
use crate::buildsystem::BuildSystem;

pub struct Bazel {
    path: PathBuf,
}

impl Bazel {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("BUILD").exists() {
            Some(Box::new(Self::new(path)))
        } else {
            None
        }
    }

    pub fn exists(path: &Path) -> bool {
        path.join("BUILD").exists()
    }
}

impl BuildSystem for Bazel {
    fn name(&self) -> &str {
        "bazel"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        todo!()
    }

    fn test(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        session.command(vec!["bazel", "test", "//..."]).run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        session.command(vec!["bazel", "build", "//..."]).run_detecting_problems()?;
        Ok(())
    }

    fn clean(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), crate::buildsystem::Error> {
        session.command(vec!["bazel", "build", "//..."]).run_detecting_problems()?;
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, crate::buildsystem::Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        todo!()
    }
}
