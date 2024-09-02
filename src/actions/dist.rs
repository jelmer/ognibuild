use crate::buildsystem::{BuildSystem, Error};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::installer::{Error as InstallerError, Installer};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use std::ffi::OsString;
use std::path::Path;

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
