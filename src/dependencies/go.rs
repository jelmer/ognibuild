use crate::dependency::{Installer, Explanation, Error, Dependency, InstallationScope};
use crate::session::Session;
use std::str::FromStr;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoPackageDependency {
    package: String,
    version: Option<String>,
}

impl GoPackageDependency {
    pub fn new(package: &str, version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            version: version.map(|s| s.to_string()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            version: None,
        }
    }
}

impl Dependency for GoPackageDependency {
    fn family(&self) -> &'static str {
        "go-package"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        unimplemented!()
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["go".to_string(), "list".to_string(), "-f".to_string()];
        if let Some(version) = &self.version {
            cmd.push(format!("{{.Version}} == {}", version));
        } else {
            cmd.push("{{.Version}}".to_string());
        }
        cmd.push(self.package.clone());
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoDependency {
    version: Option<String>,
}

impl GoDependency {
    pub fn new(version: Option<&str>) -> Self {
        Self {
            version: version.map(|s| s.to_string()),
        }
    }
}

impl Dependency for GoDependency {
    fn family(&self) -> &'static str {
        "go"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["go".to_string(), "version".to_string()];
        if let Some(version) = &self.version {
            cmd.push(format!(">={}", version));
        }
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        unimplemented!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct GoResolver {
    session: Box<dyn Session>,
}

impl GoResolver {
    pub fn new(session: Box<dyn Session>) -> Self {
        Self { session }
    }

    fn cmd(&self, reqs: &[&GoPackageDependency]) -> Vec<String> {
        let mut cmd = vec!["go".to_string(), "get".to_string()];
        for req in reqs {
            cmd.push(req.package.clone());
        }
        cmd
    }
}

impl Installer for GoResolver {
    fn explain(&self, requirement: &dyn Dependency, _scope: InstallationScope) -> Result<Explanation, Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<GoPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        Ok(Explanation {
            message: format!("Install go package {}", req.package),
            command: Some(self.cmd(&[&req])),
        })
    }

    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<GoPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[&req]);
        let (env, user) = match scope {
            InstallationScope::User=> {
                (std::collections::HashMap::new(), None)
            }
            InstallationScope::Global => {
                // TODO(jelmer): Isn't this Debian-specific?
                (std::collections::HashMap::from([("GOPATH".to_string(), "/usr/share/gocode".to_string())]), Some("root"))
            }
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        };
        crate::analyze::run_detecting_problems(
            self.session.as_ref(),
            cmd.iter().map(|s| s.as_str()).collect(),
            None,
            false,
            None, user,
            Some(env),
            None,
            None,
            None
        )?;
        Ok(())
    }
}

impl crate::dependencies::debian::IntoDebianDependency for GoDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        if let Some(version) = &self.version {
            Some(vec![crate::dependencies::debian::DebianDependency::new_with_min_version("golang-go", &version.parse().unwrap())])
        } else {
            Some(vec![crate::dependencies::debian::DebianDependency::new("golang-go")])
        }
    }
}
