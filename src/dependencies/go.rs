use crate::dependency::Dependency;
use crate::installer::{Installer, Explanation, Error, InstallationScope};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

impl crate::dependencies::debian::IntoDebianDependency for GoPackageDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt.get_packages_for_paths(
            vec![Path::new("/usr/share/gocode/src").join(regex::escape(&self.package)).join(".*").to_str().unwrap()], true, false).unwrap();
        if names.is_empty() {
            return None;
        }

        Some(names.iter().map(|name| crate::dependencies::debian::DebianDependency::new(name)).collect())
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingGoPackage {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(GoPackageDependency::simple(&self.package)))
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

pub struct GoResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> GoResolver<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
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

impl<'a> Installer for GoResolver<'a> {
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
        let mut cmd = self.session.command(cmd.iter().map(|s| s.as_str()).collect()).env(env);
        if let Some(user) = user {
            cmd = cmd.user(user);
        }
        cmd.run_detecting_problems()?;
        Ok(())
    }
}

impl crate::dependencies::debian::IntoDebianDependency for GoDependency {
    fn try_into_debian_dependency(&self, _apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        if let Some(version) = &self.version {
            Some(vec![crate::dependencies::debian::DebianDependency::new_with_min_version("golang-go", &version.parse().unwrap())])
        } else {
            Some(vec![crate::dependencies::debian::DebianDependency::new("golang-go")])
        }
    }
}
