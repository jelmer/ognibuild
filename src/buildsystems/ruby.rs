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
    /// The project directory.
    path: PathBuf,
}

/// Whether a directory looks like a Ruby gem project.
///
/// A packaged `.gem` is the original signal, but Debian source packages (and
/// upstream source trees generally) ship the unpacked sources instead: a
/// `*.gemspec` and/or a `Gemfile` at the root, with the library under `lib/`.
/// Detecting those lets the SCIP indexer run on source trees that never carry a
/// built `.gem`.
fn has_gem_markers(path: &Path) -> bool {
    let entries = match std::fs::read_dir(path) {
        Ok(entries) => entries,
        Err(_) => return false,
    };
    for entry in entries.filter_map(Result::ok) {
        let p = entry.path();
        if p.extension().unwrap_or_default() == "gem"
            || p.extension().unwrap_or_default() == "gemspec"
        {
            return true;
        }
        if p.file_name().map(|n| n == "Gemfile").unwrap_or(false) {
            return true;
        }
    }
    false
}

impl Gem {
    /// Create a new Ruby gem build system.
    ///
    /// # Arguments
    /// * `path` - Path to the project directory
    ///
    /// # Returns
    /// A new Gem instance
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Probe a directory to check if it contains a Ruby gem project.
    ///
    /// Matches either a packaged `.gem` or an unpacked source tree (a
    /// `*.gemspec` or `Gemfile`).
    ///
    /// # Arguments
    /// * `path` - Path to check
    ///
    /// # Returns
    /// Some(BuildSystem) if the directory looks like a gem project, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if has_gem_markers(path) {
            Some(Box::new(Self::new(path.to_path_buf())))
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
        if gemfiles.is_empty() {
            // The project was detected from a source tree (gemspec/Gemfile) with
            // no packaged .gem; building one from source is not implemented.
            return Err(Error::Unimplemented);
        }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_probe_gemspec() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("asciidoctor.gemspec"), b"").unwrap();
        let bs = Gem::probe(td.path()).expect("gemspec should be detected");
        assert_eq!(bs.name(), "gem");
    }

    #[test]
    fn test_probe_gemfile() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(
            td.path().join("Gemfile"),
            b"source 'https://rubygems.org'\n",
        )
        .unwrap();
        let bs = Gem::probe(td.path()).expect("Gemfile should be detected");
        assert_eq!(bs.name(), "gem");
    }

    #[test]
    fn test_probe_packaged_gem() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("foo-1.0.gem"), b"").unwrap();
        assert!(Gem::probe(td.path()).is_some());
    }

    #[test]
    fn test_probe_no_markers() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("README"), b"").unwrap();
        assert!(Gem::probe(td.path()).is_none());
    }
}
