use crate::buildsystem::{BuildSystem, Error, InstallTarget};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::installer::{Error as InstallerError, Installer, InstallationScope};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use std::path::Path;

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
            &["install"],
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
