use crate::dependencies::BinaryDependency;
use crate::dependency::{Error, InstallationScope, Installer};
use crate::session::{which, Session};
use std::path::PathBuf;

/// The category of a dependency
#[derive(Debug, Clone, PartialEq, Eq)]
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::dependency::{Error, NullInstaller};
    use crate::session::plain::PlainSession;
    use crate::session::Session;
    use std::path::PathBuf;

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
