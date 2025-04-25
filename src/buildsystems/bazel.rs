use crate::buildsystem::{BuildSystem, Error};
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// Bazel build system representation.
pub struct Bazel {
    #[allow(dead_code)]
    path: PathBuf,
}

impl Bazel {
    /// Create a new Bazel build system instance.
    ///
    /// # Arguments
    /// * `path` - Path to the Bazel project directory
    ///
    /// # Returns
    /// A new Bazel instance
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Probe a directory to check if it contains a Bazel build system.
    ///
    /// # Arguments
    /// * `path` - Path to check for Bazel build files
    ///
    /// # Returns
    /// Some(BuildSystem) if a Bazel build is found, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("BUILD").exists() {
            Some(Box::new(Self::new(path)))
        } else {
            None
        }
    }

    /// Check if a Bazel build system exists at the specified path.
    ///
    /// # Arguments
    /// * `path` - Path to check for Bazel build files
    ///
    /// # Returns
    /// true if a BUILD file exists, false otherwise
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
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["bazel", "test", "//..."])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["bazel", "build", "//..."])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["bazel", "build", "//..."])
            .run_detecting_problems()?;
        Err(Error::Unimplemented)
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
        crate::buildsystem::Error,
    > {
        Err(Error::Unimplemented)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
