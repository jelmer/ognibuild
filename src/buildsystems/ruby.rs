//! Support for Ruby build systems.
//!
//! This module provides functionality for building, testing, and installing
//! Ruby gems using the gem command.

use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use std::path::{Path, PathBuf};

/// Ruby gem build system.
///
/// This build system handles Ruby gems for distribution and installation.
#[derive(Debug)]
pub struct Gem {
    path: PathBuf,
}

impl Gem {
    /// Create a new Ruby gem build system.
    ///
    /// # Arguments
    /// * `path` - Path to the gem file
    ///
    /// # Returns
    /// A new Gem instance
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Probe a directory to check if it contains Ruby gem files.
    ///
    /// # Arguments
    /// * `path` - Path to check for gem files
    ///
    /// # Returns
    /// Some(BuildSystem) if gem files are found, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let mut gemfiles = std::fs::read_dir(path)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().unwrap_or_default() == "gem")
            .collect::<Vec<_>>();
        if !gemfiles.is_empty() {
            Some(Box::new(Self::new(gemfiles.remove(0))))
        } else {
            None
        }
    }
}

/// Implementation of BuildSystem for Ruby gems.
impl BuildSystem for Gem {
    /// Get the name of this build system.
    ///
    /// # Returns
    /// The string "gem"
    fn name(&self) -> &str {
        "gem"
    }

    /// Create a distribution package from the gem file.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    /// * `target_directory` - Directory to store the created distribution package
    /// * `quiet` - Whether to suppress output
    ///
    /// # Returns
    /// OsString with the name of the created distribution package, or an error
    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        let mut gemfiles = std::fs::read_dir(&self.path)
            .unwrap()
            .filter_map(|entry| entry.ok().map(|entry| entry.path()))
            .filter(|path| path.extension().unwrap_or_default() == "gem")
            .collect::<Vec<_>>();
        assert!(!gemfiles.is_empty());
        if gemfiles.len() > 1 {
            log::warn!("More than one gemfile. Trying the first?");
        }
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(Path::new(".")));
        session
            .command(vec![
                guaranteed_which(session, installer, "gem2tgz")?
                    .to_str()
                    .unwrap(),
                gemfiles.remove(0).to_str().unwrap(),
            ])
            .quiet(quiet)
            .run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    /// Run tests for this gem.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as testing is not implemented for gems
    fn test(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Build this gem.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as building is not implemented for gems
    fn build(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Clean build artifacts.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as cleaning is not implemented for gems
    fn clean(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Install the gem.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    /// * `_install_target` - Target installation directory
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as installation is not implemented for gems
    fn install(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Get dependencies declared by this gem.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as dependency discovery is not implemented for gems
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
        Err(Error::Unimplemented)
    }

    /// Get outputs declared by this gem.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as output discovery is not implemented for gems
    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        Err(Error::Unimplemented)
    }

    /// Convert this build system to Any for downcasting.
    ///
    /// # Returns
    /// Reference to self as Any
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
