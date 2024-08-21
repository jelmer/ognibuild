use crate::session::Session;
use breezyshim::tree::Tree;
use std::path::Path;
use crate::logs::{NoLogManager, LogManager};
use crate::buildsystem::detect_buildsystems;
use crate::buildlog::InstallFixer;
use crate::fix_build::{BuildFixer, iterate_with_build_fixers};
use crate::fixers::{
    GitIdentityFixer,
    GnulibDirectoryFixer,
    MinimumAutoconfFixer,
    MissingGoSumEntryFixer,
    SecretGpgKeyFixer,
    UnexpandedAutoconfMacroFixer,
};


// This is the function used by debianize()
/// Create a dist tarball for a tree.
///
/// # Arguments
/// * `session` - session to run it
/// * `tree` - Tree object to work in
/// * `target_dir` - Directory to write tarball into
/// * `include_controldir` - Whether to include the version control directory
/// * `subdir` - subdirectory in the tree to operate in
/// * `log_manager` - log manager to use
/// * `version` - version to use
/// * `subpath` - subpath to use
pub fn create_dist<'a>(
    session: &'a dyn Session,
    tree: &'a dyn Tree,
    target_dir: &'a Path,
    include_controldir: bool,
    subdir: Option<&'a Path>,
    log_manager: Option<&'a dyn LogManager>,
    version: Option<&'a str>,
    subpath: &'a Path,
) -> Option<&'a str> {
    let subdir = subdir.unwrap_or(Path::new("package"));
    let (export_directory, reldir) = session.setup_from_vcs(
            tree, include_controldir, subdir
        );

    let log_manager = log_manager.unwrap_or(NoLogManager);

    dist(
        session,
        export_directory.join(subpath),
        reldir.join(subpath),
        target_dir,
        log_manager,
        version,
    )
}

pub fn dist(
    session: &dyn Session,
    export_directory: &Path,
    reldir: &Path,
    target_dir: &Path,
    log_manager: &dyn LogManager,
    version: Option<&str>,
    quiet: bool,
) {
    if let Some(version) = version {
        // TODO(jelmer): Shouldn't include backend-specific code here
        os.environ["SETUPTOOLS_SCM_PRETEND_VERSION"] = version
    }

    // TODO(jelmer): use scan_buildsystems to also look in subdirectories
    let buildsystems = detect_buildsystems(export_directory).collect::<Vec<_>>();
    let installer = crate::installer::auto_installer(session, false, None, None);
    let fixers: Vec<Box<dyn crate::fix_build::BuildFixer>> = vec![
        Box::new(UnexpandedAutoconfMacroFixer::new(session, installer)),
        Box::new(GnulibDirectoryFixer::new(session)),
        Box::new(MinimumAutoconfFixer::new(session)),
        Box::new(MissingGoSumEntryFixer::new(session)),
        Box::new(InstallFixer::new(installer)),
    ];

    if session.is_temporary() {
        // Only muck about with temporary sessions
        fixers.extend(
            [
                Box::new(GitIdentityFixer::new(session)),
                Box::new(SecretGpgKeyFixer::new(session))
            ]
        );
    }

    session.chdir(reldir);

    // Some things want to write to the user's home directory, e.g. pip caches in ~/.cache
    session.create_home();

    log::info!("Using dependency installer: {:?}", installer);

    for buildsystem in buildsystems {
        return iterate_with_build_fixers(
            fixers,
            &["dist"],
            log_manager.wrap(
                partial(
                    buildsystem.dist(
                    session,
                    resolver,
                    target_dir,
                    quiet=quiet,
                    )
                )
            ),
            None
        );
    }

    raise NoBuildToolsFound()
}
