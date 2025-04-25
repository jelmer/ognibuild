use crate::buildsystem::{BuildSystem, Error};
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// Haskell Cabal build system representation.
pub struct Cabal {
    #[allow(dead_code)]
    path: PathBuf,
}

impl Cabal {
    /// Create a new Cabal build system instance.
    ///
    /// # Arguments
    /// * `path` - Path to the Cabal project directory
    ///
    /// # Returns
    /// A new Cabal instance
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Run a Cabal command with the given arguments.
    ///
    /// Handles common Cabal errors, such as needing to run configure first.
    ///
    /// # Arguments
    /// * `session` - The session to run the command in
    /// * `extra_args` - Additional arguments to pass to the Cabal command
    ///
    /// # Returns
    /// Ok(()) if the command succeeded, otherwise an error
    fn run(
        &self,
        session: &dyn crate::session::Session,
        extra_args: Vec<&str>,
    ) -> Result<(), crate::analyze::AnalyzedError> {
        let mut args = vec!["runhaskell", "Setup.hs"];
        args.extend(extra_args);
        match session.command(args.clone()).run_detecting_problems() {
            Ok(ls) => Ok(ls),
            Err(crate::analyze::AnalyzedError::Unidentified { lines, .. })
                if lines.contains(&"Run the 'configure' command first.".to_string()) =>
            {
                session
                    .command(vec!["runhaskell", "Setup.hs", "configure"])
                    .run_detecting_problems()?;
                session.command(args).run_detecting_problems()
            }
            Err(e) => Err(e),
        }
        .map(|_| ())
    }

    /// Probe a directory to check if it contains a Cabal project.
    ///
    /// # Arguments
    /// * `path` - Path to check for Cabal project files
    ///
    /// # Returns
    /// Some(BuildSystem) if a Cabal project is found, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("Setup.hs").exists() {
            Some(Box::new(Self::new(path.to_owned())))
        } else {
            None
        }
    }
}

impl BuildSystem for Cabal {
    fn name(&self) -> &str {
        "cabal"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        let dc = crate::dist_catcher::DistCatcher::new(vec![
            session.external_path(Path::new("dist-newstyle/sdist")),
            session.external_path(Path::new("dist")),
        ]);
        self.run(session, vec!["sdist"])?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.run(session, vec!["test"])?;
        Ok(())
    }

    fn build(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
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
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
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
