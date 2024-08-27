use crate::dependencies::BinaryDependency;
use crate::dependency::Dependency;
use crate::installer::{Error, InstallationScope, Installer, install_missing_deps};
use crate::session::{which, Session};
use std::path::{Path, PathBuf};

/// The category of a dependency
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum DependencyCategory {
    /// A dependency that is required for the package to build
    Universal,
    /// Building of artefacts
    Build,
    /// For running artefacts after build or install
    Runtime,
    /// Test infrastructure, e.g. test frameworks or test runners
    Test,
    /// Needed for development, e.g. linters or IDE plugins
    Dev,
}

impl DependencyCategory {
    pub fn all() -> [DependencyCategory; 5] {
        [
            DependencyCategory::Universal,
            DependencyCategory::Build,
            DependencyCategory::Runtime,
            DependencyCategory::Test,
            DependencyCategory::Dev,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Clean,
    Build,
    Test,
    Install,
}

/// Determine the path to a binary, installing it if necessary
pub fn guaranteed_which(
    session: &dyn Session,
    installer: &dyn Installer,
    name: &str,
) -> Result<PathBuf, Error> {
    match which(session, name) {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            installer.install(&BinaryDependency::new(name), InstallationScope::Global)?;
            Ok(PathBuf::from(which(session, name).unwrap()))
        }
    }
}

/// A particular buildsystem.
pub trait BuildSystem {
    /// The name of the buildsystem.
    fn name(&self) -> &str;

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<(), String>;

    fn install_declared_dependencies(
        &self,
        categories: &[DependencyCategory],
        scope: InstallationScope,
        session: &dyn Session,
        installer: &dyn Installer,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<Error>]>,
    ) -> Result<(), Error> {
        let declared_deps = self.get_declared_dependencies(session, fixers);
        let relevant =
            declared_deps.into_iter().filter(|(c, _d)| categories.contains(c)).map(|(_, d)| d).collect::<Vec<_>>();
        install_missing_deps(session, installer, scope, &relevant)?;
        Ok(())
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), String>;

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), String>;

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), String>;

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &Path,
    ) -> Result<(), String>;

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<Error>]>,
    ) -> Vec<(DependencyCategory, &dyn Dependency)>;

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<Error>]>,
    ) -> Vec<PathBuf>;
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::{Error, NullInstaller};
    use crate::session::plain::PlainSession;

    #[test]
    fn test_guaranteed_which() {
        let session = PlainSession::new();
        let installer = NullInstaller::new();

        let _path = guaranteed_which(&session, &installer, "ls").unwrap();
    }

    #[test]
    fn test_guaranteed_which_not_found() {
        let session = PlainSession::new();
        let installer = NullInstaller::new();

        assert!(matches!(
            guaranteed_which(&session, &installer, "this-does-not-exist").unwrap_err(),
            Error::UnknownDependencyFamily,
        ));
    }
}
