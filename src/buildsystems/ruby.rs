use crate::buildsystem::{BuildSystem, Error, guaranteed_which};
use crate::analyze::run_detecting_problems;
use std::path::{Path, PathBuf};

pub struct Gem {
    path: PathBuf,
}

impl Gem {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let mut gemfiles = std::fs::read_dir(path)
            .unwrap()
            .filter_map(|entry| {
                entry.ok().map(|entry| entry.path())
            })
            .filter(|path| path.extension().unwrap_or_default() == "gem")
            .collect::<Vec<_>>();
        if !gemfiles.is_empty() {
            Some(Box::new(Self::new(gemfiles.remove(0))))
        } else {
            None
        }
    }
}

impl BuildSystem for Gem {
    fn name(&self) -> &str {
        "gem"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        let mut gemfiles = std::fs::read_dir(&self.path)
            .unwrap()
            .filter_map(|entry| {
                entry.ok().map(|entry| entry.path())
            })
            .filter(|path| path.extension().unwrap_or_default() == "gem")
            .collect::<Vec<_>>();
        assert!(!gemfiles.is_empty());
        if gemfiles.len() > 1 {
            log::warn!("More than one gemfile. Trying the first?");
        }
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(Path::new(".")));
        session.command(
            vec![guaranteed_which(session, installer, "gem2tgz")?.to_str().unwrap(), gemfiles.remove(0).to_str().unwrap()],
        ).quiet(quiet).run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        todo!()
    }

    fn build(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        todo!()
    }

    fn clean(&self, session: &dyn crate::session::Session, installer: &dyn crate::installer::Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        todo!()
    }
}
