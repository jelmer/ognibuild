use crate::session::Session;
use crate::dependency::Dependency;
use crate::installer::{Installer, Explanation, Error, InstallationScope};
use crate::analyze::run_detecting_problems;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RPackageDependency {
    package: String,
    minimum_version: Option<String>,
}

impl RPackageDependency {
    pub fn new(package: &str, minimum_version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: None,
        }
    }
}

impl Dependency for RPackageDependency {
    fn family(&self) -> &'static str {
        "r-package"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::dependencies::debian::IntoDebianDependency for RPackageDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> Option<Vec<super::debian::DebianDependency>> {
        let names = apt.get_packages_for_paths(vec![
            std::path::Path::new("/usr/lib/R/site-library").join(&self.package).join("DESCRIPTION").to_str().unwrap()
        ], false, false).unwrap();

        if names.is_empty() {
            return None;
        }

        Some(names.into_iter().map(|name| super::debian::DebianDependency::new(&name)).collect())
    }
}

pub struct RResolver {
    session: Box<dyn Session>,
    repos: String,
}

impl RResolver {
    pub fn new(session: Box<dyn Session>, repos: &str) -> Self {
        Self {
            session,
            repos: repos.to_string(),
        }
    }

    fn cmd(&self, req: &RPackageDependency) -> Vec<String> {
        // R will install into the first directory in .libPaths() that is writeable.
        // TODO: explicitly set the library path to either the user's home directory or a system
        // directory.
        vec![
            "R".to_string(),
            "-e".to_string(),
            format!("install.packages('{}', repos='{})'", req.package, self.repos),
        ]
    }
}

impl Installer for RResolver {
    /// Install the dependency into the session.
    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = dep.as_any().downcast_ref::<RPackageDependency>().ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(req);
        let user = match scope {
            InstallationScope::User => None,
            InstallationScope::Global => Some("root"),
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        };
        log::info!("RResolver({:?}): running {:?}", self.repos, cmd);
        run_detecting_problems(self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect() , None, false, None, user, None, None, None, None)?;
        Ok(())
    }

    /// Explain how to install the dependency.
    fn explain(&self, dep: &dyn Dependency, _scope: InstallationScope) -> Result<Explanation, Error> {
        if let Some(req) = dep.as_any().downcast_ref::<RPackageDependency>() {
            Ok(Explanation {
                message: format!("Install R package {}", req.package),
                command: Some(self.cmd(req)),
            })
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }
}

pub fn bioconductor(session: Box<dyn Session>) -> Box<dyn Installer> {
    Box::new(RResolver::new(session, "https://hedgehog.fhcrc.org/bioconductor"))
}

pub fn cran(session: Box<dyn Session>) -> Box<dyn Installer> {
    Box::new(RResolver::new(session, "https://cran.r-project.org"))
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingRPackage {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RPackageDependency::simple(&self.package)))
    }
}
