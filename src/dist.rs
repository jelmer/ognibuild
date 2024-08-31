use crate::buildsystem::{detect_buildsystems, BuildSystem, Error};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError, IterateBuildError};
use crate::fixers::*;
use crate::installer::{auto_installer, Error as InstallerError, InstallationScope, Installer};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use std::ffi::OsString;
use std::path::Path;

pub fn dist(
    session: &mut dyn Session,
    export_directory: &Path,
    reldir: &Path,
    target_dir: &Path,
    log_manager: &mut dyn LogManager,
    version: Option<&str>,
    quiet: bool,
) -> Result<OsString, Error> {
    session.chdir(reldir)?;

    if let Some(version) = version {
        // TODO(jelmer): Shouldn't include backend-specific code here
        std::env::set_var("SETUPTOOLS_SCM_PRETEND_VERSION", version);
    }

    // TODO(jelmer): use scan_buildsystems to also look in subdirectories
    let buildsystems = detect_buildsystems(export_directory);
    let installer = auto_installer(session, false, None, None);
    let mut fixers: Vec<Box<dyn BuildFixer<InstallerError>>> = vec![
        Box::new(UnexpandedAutoconfMacroFixer::new(
            session,
            installer.as_ref(),
        )),
        Box::new(GnulibDirectoryFixer::new(session)),
        Box::new(MinimumAutoconfFixer::new(session)),
        Box::new(MissingGoSumEntryFixer::new(session)),
        Box::new(InstallFixer::new(
            installer.as_ref(),
            InstallationScope::User,
        )),
    ];

    if session.is_temporary() {
        // Only muck about with temporary sessions
        fixers.extend([
            Box::new(GitIdentityFixer::new(session)) as Box<dyn BuildFixer<InstallerError>>,
            Box::new(SecretGpgKeyFixer::new(session)) as Box<dyn BuildFixer<InstallerError>>,
        ]);
    }

    // Some things want to write to the user's home directory, e.g. pip caches in ~/.cache
    session.create_home()?;

    for buildsystem in buildsystems {
        return Ok(iterate_with_build_fixers(
            fixers
                .iter()
                .map(|x| x.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
            &["dist"],
            || -> Result<_, InterimError<Error>> {
                Ok(wrap(log_manager, || -> Result<_, Error> {
                    Ok(buildsystem.dist(session, installer.as_ref(), target_dir, quiet)?)
                })?)
            },
            None,
        )?);
    }

    Err(Error::NoBuildSystemDetected)
}
