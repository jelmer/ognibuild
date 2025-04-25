use crate::buildsystem::{BuildSystem, Error};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::installer::{Error as InstallerError, Installer};
use crate::logs::{wrap, LogManager};
use crate::session::Session;

/// Run the build process using the first applicable build system.
///
/// This function attempts to build a package using the first build system in the provided list
/// that is applicable for the current project. If the build fails, it will attempt to fix
/// issues using the provided fixers.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of build systems to try
/// * `installer` - Installer to use for installing dependencies
/// * `fixers` - List of fixers to try if build fails
/// * `log_manager` - Manager for logging build output
///
/// # Returns
/// * `Ok(())` if the build succeeds
/// * `Err(Error::NoBuildSystemDetected)` if no build system could be used
/// * Other errors if the build fails and can't be fixed
pub fn run_build(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    log_manager: &mut dyn LogManager,
) -> Result<(), Error> {
    // Some things want to write to the user's home directory, e.g. pip caches in ~/.cache
    session.create_home()?;

    for buildsystem in buildsystems {
        return Ok(iterate_with_build_fixers(
            fixers,
            || -> Result<_, InterimError<Error>> {
                Ok(wrap(log_manager, || -> Result<_, Error> {
                    Ok(buildsystem.build(session, installer)?)
                })?)
            },
            None,
        )?);
    }

    Err(Error::NoBuildSystemDetected)
}
