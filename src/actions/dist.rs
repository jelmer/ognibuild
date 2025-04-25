use crate::buildsystem::{BuildSystem, Error};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::installer::{Error as InstallerError, Installer};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use std::ffi::OsString;
use std::path::Path;

/// Run the distribution package creation process using the first applicable build system.
///
/// This function attempts to create a distribution package using the first build system in the
/// provided list that is applicable for the current project. If the operation fails, it will
/// attempt to fix issues using the provided fixers.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of build systems to try
/// * `installer` - Installer to use for installing dependencies
/// * `fixers` - List of fixers to try if dist creation fails
/// * `target_directory` - Directory where distribution packages should be created
/// * `quiet` - Whether to suppress output
/// * `log_manager` - Manager for logging output
///
/// # Returns
/// * `Ok(OsString)` with the filename of the created package if successful
/// * `Err(Error::NoBuildSystemDetected)` if no build system could be used
/// * Other errors if the dist creation fails and can't be fixed
pub fn run_dist(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    target_directory: &Path,
    quiet: bool,
    log_manager: &mut dyn LogManager,
) -> Result<OsString, Error> {
    // Some things want to write to the user's home directory, e.g. pip caches in ~/.cache
    session.create_home()?;

    for buildsystem in buildsystems {
        return Ok(iterate_with_build_fixers(
            fixers,
            || -> Result<_, InterimError<Error>> {
                Ok(wrap(log_manager, || -> Result<_, Error> {
                    Ok(buildsystem.dist(session, installer, target_directory, quiet)?)
                })?)
            },
            None,
        )?);
    }

    Err(Error::NoBuildSystemDetected)
}
