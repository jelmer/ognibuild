use crate::buildsystem::{BuildSystem, Error, InstallTarget};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::installer::{Error as InstallerError, InstallationScope, Installer};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use std::path::Path;

/// Run the installation process using the first applicable build system.
///
/// This function attempts to install a package using the first build system in the provided list
/// that is applicable for the current project. If the installation fails, it will attempt to fix
/// issues using the provided fixers.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of build systems to try
/// * `installer` - Installer to use for installing dependencies
/// * `fixers` - List of fixers to try if installation fails
/// * `log_manager` - Manager for logging installation output
/// * `scope` - Installation scope (user or system)
/// * `prefix` - Optional installation prefix path
///
/// # Returns
/// * `Ok(())` if the installation succeeds
/// * `Err(Error::NoBuildSystemDetected)` if no build system could be used
/// * Other errors if the installation fails and can't be fixed
pub fn run_install(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    log_manager: &mut dyn LogManager,
    scope: InstallationScope,
    prefix: Option<&Path>,
) -> Result<(), Error> {
    // Some things want to write to the user's home directory, e.g. pip caches in ~/.cache
    session.create_home()?;

    let target = InstallTarget {
        scope,
        prefix: prefix.map(|p| p.to_path_buf()),
    };

    for buildsystem in buildsystems {
        return Ok(iterate_with_build_fixers(
            fixers,
            || -> Result<_, InterimError<Error>> {
                Ok(wrap(log_manager, || -> Result<_, Error> {
                    Ok(buildsystem.install(session, installer, &target)?)
                })?)
            },
            None,
        )?);
    }

    Err(Error::NoBuildSystemDetected)
}
