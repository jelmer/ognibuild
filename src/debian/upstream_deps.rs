use crate::buildsystem::{BuildSystem, DependencyCategory};
use crate::dependencies::debian::DebianDependency;
use crate::installer::Error as InstallerError;
use crate::session::Session;

/// Get the project-wide dependencies for a project.
///
/// This function will return a tuple of two vectors of `DebianDependency` objects. The first
/// vector will contain the build dependencies, and the second vector will contain the test
/// dependencies.
///
/// # Arguments
/// * `session` - The session to use for the operation.
/// * `buildsystem` - The build system to use for the operation.
pub fn get_project_wide_deps(
    session: &dyn Session,
    buildsystem: &dyn BuildSystem,
) -> (Vec<DebianDependency>, Vec<DebianDependency>) {
    let mut build_deps = vec![];
    let mut test_deps = vec![];

    let apt = crate::debian::apt::AptManager::new(session, None);

    let apt_installer = crate::debian::apt::AptInstaller::new(apt);

    let scope = crate::installer::InstallationScope::Global;

    let build_fixers = [
        Box::new(crate::fixers::InstallFixer::new(&apt_installer, scope))
            as Box<dyn crate::fix_build::BuildFixer<InstallerError>>,
    ];

    let apt = crate::debian::apt::AptManager::new(session, None);

    // Try to create build dependency tie breaker, but handle failure gracefully
    let mut tie_breakers = vec![];
    match crate::debian::build_deps::BuildDependencyTieBreaker::try_from_session(session) {
        Ok(tie_breaker) => {
            tie_breakers
                .push(Box::new(tie_breaker) as Box<dyn crate::dependencies::debian::TieBreaker>);
        }
        Err(e) => {
            log::warn!(
                "Failed to create BuildDependencyTieBreaker: {}. Using basic dependency resolution.",
                e
            );
        }
    }

    #[cfg(feature = "udd")]
    {
        tie_breakers.push(Box::new(crate::debian::udd::PopconTieBreaker)
            as Box<dyn crate::dependencies::debian::TieBreaker>);
    }
    match buildsystem.get_declared_dependencies(
        session,
        Some(
            build_fixers
                .iter()
                .map(|x| x.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        ),
    ) {
        Err(e) => {
            log::error!("Unable to obtain declared dependencies: {}", e);
        }
        Ok(upstream_deps) => {
            for (kind, dep) in upstream_deps {
                let apt_dep = crate::debian::apt::dependency_to_deb_dependency(
                    &apt,
                    dep.as_ref(),
                    tie_breakers.as_slice(),
                )
                .unwrap();
                if apt_dep.is_none() {
                    log::warn!(
                        "Unable to map upstream requirement {:?} (kind {}) to a Debian package",
                        dep,
                        kind,
                    );
                    continue;
                }
                let apt_dep = apt_dep.unwrap();
                log::debug!("Mapped {:?} (kind: {}) to {:?}", dep, kind, apt_dep);
                if [DependencyCategory::Universal, DependencyCategory::Build].contains(&kind) {
                    build_deps.push(apt_dep.clone());
                }
                if [DependencyCategory::Universal, DependencyCategory::Test].contains(&kind) {
                    test_deps.push(apt_dep.clone());
                }
            }
        }
    }
    (build_deps, test_deps)
}
