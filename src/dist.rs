use crate::buildsystem::{detect_buildsystems, Error};
use crate::fix_build::{iterate_with_build_fixers, BuildFixer, InterimError};
use crate::fixers::*;
use crate::installer::{
    auto_installation_scope, auto_installer, Error as InstallerError, InstallationScope,
};
use crate::logs::{wrap, LogManager};
use crate::session::Session;
use breezyshim::tree::Tree;
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
    let scope = auto_installation_scope(session);
    let installer = auto_installer(session, scope, None);
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

// This is the function used by debianize()
/// Create a dist tarball for a tree.
///
/// # Arguments
/// * `session` - session to run it
/// * `tree` - Tree object to work in
/// * `target_dir` - Directory to write tarball into
/// * `include_controldir` - Whether to include the version control directory
/// * `temp_subdir` - name of subdirectory in which to check out the source code;
///            defaults to "package"
pub fn create_dist<T: crate::vcs::DupableTree>(
    session: &mut dyn Session,
    tree: &T,
    target_dir: &Path,
    include_controldir: Option<bool>,
    log_manager: &mut dyn LogManager,
    version: Option<&str>,
    subpath: &Path,
    temp_subdir: Option<&str>,
) -> Result<OsString, Error> {
    let temp_subdir = temp_subdir.unwrap_or("package");

    let (export_directory, reldir) =
        session.setup_from_vcs(tree, include_controldir, Some(Path::new(temp_subdir)))?;

    dist(
        session,
        &export_directory.join(subpath),
        &reldir.join(subpath),
        target_dir,
        log_manager,
        version,
        false,
    )
}

#[cfg(target_os = "linux")]
/// Create a dist tarball for a tree.
///
/// # Arguments
/// * `session` - session to run it
/// * `tree` - Tree object to work in
/// * `target_dir` - Directory to write tarball into
/// * `include_controldir` - Whether to include the version control directory
/// * `temp_subdir` - name of subdirectory in which to check out the source code;
///             defaults to "package"
pub fn create_dist_schroot<T: crate::vcs::DupableTree>(
    tree: &T,
    target_dir: &Path,
    chroot: &str,
    packaging_tree: Option<&dyn Tree>,
    packaging_subpath: Option<&Path>,
    include_controldir: Option<bool>,
    subpath: &Path,
    log_manager: &mut dyn LogManager,
    version: Option<&str>,
    temp_subdir: Option<&str>,
) -> Result<OsString, Error> {
    // TODO(jelmer): pass in package name as part of session prefix
    let mut session = crate::session::schroot::SchrootSession::new(chroot, Some("ognibuild-dist"))?;
    if let (Some(packaging_tree), Some(packaging_subpath)) = (packaging_tree, packaging_subpath) {
        crate::debian::satisfy_build_deps(&session, packaging_tree, packaging_subpath)
            .map_err(|e| Error::Other(format!("Failed to satisfy build dependencies: {:?}", e)))?;
    }
    create_dist(
        &mut session,
        tree,
        target_dir,
        include_controldir,
        log_manager,
        version,
        subpath,
        temp_subdir,
    )
}
