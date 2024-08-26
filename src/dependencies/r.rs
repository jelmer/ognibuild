use crate::session::Session;
use crate::dependency::{Dependency, Installer, Explanation, Error, InstallationScope};
use crate::analyze::{run_detecting_problems, AnalyzedError};
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

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
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
    fn explain(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
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
