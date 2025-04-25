//! Support for Waf build systems.
//!
//! This module provides functionality for building, testing, and distributing
//! software that uses the Waf build system.

use crate::buildsystem::{BuildSystem, Error};
use crate::dependency::Dependency;
use crate::installer::{InstallationScope, Installer};
use crate::session::Session;
use std::path::PathBuf;

/// Waf build system.
///
/// This build system handles projects that use Waf for building and testing.
#[derive(Debug)]
pub struct Waf {
    #[allow(dead_code)]
    path: PathBuf,
}

impl Waf {
    /// Create a new Waf build system.
    ///
    /// # Arguments
    /// * `path` - Path to the waf script
    ///
    /// # Returns
    /// A new Waf instance
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Set up the environment for using Waf.
    ///
    /// Ensures Python 3 is installed as it's required by Waf.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn setup(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let binary_req = crate::dependencies::BinaryDependency::new("python3");
        if !binary_req.present(session) {
            installer.install(&binary_req, InstallationScope::Global)?;
        }
        Ok(())
    }

    /// Probe a directory to check if it contains a Waf build system.
    ///
    /// # Arguments
    /// * `path` - Path to check for a waf script
    ///
    /// # Returns
    /// Some(BuildSystem) if a waf script is found, None otherwise
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

/// Implementation of BuildSystem for Waf.
impl BuildSystem for Waf {
    /// Get the name of this build system.
    ///
    /// # Returns
    /// The string "waf"
    fn name(&self) -> &str {
        "waf"
    }

    /// Create a distribution package using waf dist command.
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
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session, installer)?;
        let dc = crate::dist_catcher::DistCatcher::default(
            &session.external_path(std::path::Path::new(".")),
        );
        session
            .command(vec!["./waf", "dist"])
            .quiet(quiet)
            .run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    /// Run tests using waf test command.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer)?;
        session
            .command(vec!["./waf", "test"])
            .run_detecting_problems()?;
        Ok(())
    }

    /// Build the project using waf build command.
    ///
    /// Automatically runs configure if necessary.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer)?;
        match session
            .command(vec!["./waf", "build"])
            .run_detecting_problems()
        {
            Err(crate::analyze::AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"The project was not configured: run \"waf configure\" first!".to_string(),
                ) =>
            {
                session
                    .command(vec!["./waf", "configure"])
                    .run_detecting_problems()?;
                session
                    .command(vec!["./waf", "build"])
                    .run_detecting_problems()
            }
            other => other,
        }?;
        Ok(())
    }

    /// Clean build artifacts.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as cleaning is not implemented for Waf
    fn clean(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Install the built software.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    /// * `_install_target` - Target installation directory
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as installation is not implemented for Waf
    fn install(
        &self,
        _session: &dyn Session,
        _installer: &dyn Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    /// Get dependencies declared by this project.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as dependency discovery is not implemented for Waf
    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
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

    /// Get outputs declared by this project.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as output discovery is not implemented for Waf
    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
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
