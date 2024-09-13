use crate::buildsystem::{BuildSystem, DependencyCategory};
use crate::dependencies::debian::DebianDependency;
use crate::installer::Error as InstallerError;
use crate::session::{Project, Session};
use std::path::Path;

pub fn get_project_wide_deps(
    session: &mut dyn Session,
    project: &Project,
    subpath: &Path,
    buildsystem: &dyn BuildSystem,
    buildsystem_subpath: &Path,
) -> (Vec<DebianDependency>, Vec<DebianDependency>) {
    let mut build_deps = vec![];
    let mut test_deps = vec![];

    session.chdir(project.internal_path()).unwrap();

    let apt = crate::debian::apt::AptManager::new(session, None);

    let apt_installer = crate::debian::apt::AptInstaller::new(apt);

    let scope = crate::installer::InstallationScope::Global;

    let build_fixers = [
        Box::new(crate::fixers::InstallFixer::new(&apt_installer, scope))
            as Box<dyn crate::fix_build::BuildFixer<InstallerError>>,
    ];

    let apt = crate::debian::apt::AptManager::new(session, None);
    let tie_breakers = vec![
        Box::new(crate::debian::build_deps::BuildDependencyTieBreaker::from_session(session))
            as Box<dyn crate::dependencies::debian::TieBreaker>,
        #[cfg(feature = "udd")]
        {
            Box::new(crate::debian::udd::PopconTieBreaker)
                as Box<dyn crate::dependencies::debian::TieBreaker>
        },
    ];
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
