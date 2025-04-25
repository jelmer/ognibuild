use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a Go package dependency.
pub struct GoPackageDependency {
    /// The name of the Go package.
    pub package: String,

    /// The version of the Go package, if specified.
    pub version: Option<String>,
}

impl GoPackageDependency {
    /// Creates a new `GoPackageDependency` instance.
    pub fn new(package: &str, version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            version: version.map(|s| s.to_string()),
        }
    }

    /// Creates a simple `GoPackageDependency` instance without a version.
    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            version: None,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;
    use std::any::Any;

    #[test]
    fn test_go_package_dependency_new() {
        let dependency = GoPackageDependency::new("github.com/pkg/errors", Some("v0.9.1"));
        assert_eq!(dependency.package, "github.com/pkg/errors");
        assert_eq!(dependency.version, Some("v0.9.1".to_string()));
    }

    #[test]
    fn test_go_package_dependency_simple() {
        let dependency = GoPackageDependency::simple("github.com/pkg/errors");
        assert_eq!(dependency.package, "github.com/pkg/errors");
        assert_eq!(dependency.version, None);
    }

    #[test]
    fn test_go_package_dependency_family() {
        let dependency = GoPackageDependency::simple("github.com/pkg/errors");
        assert_eq!(dependency.family(), "go-package");
    }

    #[test]
    fn test_go_package_dependency_as_any() {
        let dependency = GoPackageDependency::simple("github.com/pkg/errors");
        let any_dep: &dyn Any = dependency.as_any();
        assert!(any_dep.downcast_ref::<GoPackageDependency>().is_some());
    }

    #[test]
    fn test_missing_go_package_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingGoPackage {
            package: "github.com/pkg/errors".to_string(),
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "go-package");
        let go_dep = dep.as_any().downcast_ref::<GoPackageDependency>().unwrap();
        assert_eq!(go_dep.package, "github.com/pkg/errors");
    }

    #[test]
    fn test_go_dependency_new() {
        let dependency = GoDependency::new(Some("1.16"));
        assert_eq!(dependency.version, Some("1.16".to_string()));
    }

    #[test]
    fn test_go_dependency_family() {
        let dependency = GoDependency::new(None);
        assert_eq!(dependency.family(), "go");
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

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for GoPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![Path::new("/usr/share/gocode/src")
                    .join(regex::escape(&self.package))
                    .join(".*")
                    .to_str()
                    .unwrap()],
                true,
                false,
            )
            .unwrap();
        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|name| crate::dependencies::debian::DebianDependency::new(name))
                .collect(),
        )
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::FromDebianDependency for GoPackageDependency {
    fn from_debian_dependency(
        dependency: &super::debian::DebianDependency,
    ) -> Option<Box<dyn Dependency>> {
        let (package, version) =
            crate::dependencies::debian::extract_simple_exact_version(&dependency)?;
        let (_, package) = lazy_regex::regex_captures!(r"golang-(.*)-dev", &package)?;

        let mut parts = package.split('-').collect::<Vec<_>>();

        if parts[0] == "github" {
            parts[1] = "github.com";
        }
        if parts[0] == "gopkg" {
            parts[1] = "gopkg.in";
        }

        Some(Box::new(GoPackageDependency::new(
            &parts.join("/"),
            version.map(|s| s.to_string()).as_deref(),
        )))
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingGoPackage {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(GoPackageDependency::simple(&self.package)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// Represents a Go dependency.
pub struct GoDependency {
    /// The version of the Go dependency, if specified.
    pub version: Option<String>,
}

impl GoDependency {
    /// Creates a new `GoDependency` instance.
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

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for GoPackageDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        upstream_ontologist::providers::go::remote_go_metadata(&self.package).ok()
    }
}

/// A resolver for Go package dependencies.
pub struct GoResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> GoResolver<'a> {
    /// Creates a new `GoResolver` instance.
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
    fn explain(
        &self,
        requirement: &dyn Dependency,
        _scope: InstallationScope,
    ) -> Result<Explanation, Error> {
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
            InstallationScope::User => (std::collections::HashMap::new(), None),
            InstallationScope::Global => {
                // TODO(jelmer): Isn't this Debian-specific?
                (
                    std::collections::HashMap::from([(
                        "GOPATH".to_string(),
                        "/usr/share/gocode".to_string(),
                    )]),
                    Some("root"),
                )
            }
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        };
        let mut cmd = self
            .session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .env(env);
        if let Some(user) = user {
            cmd = cmd.user(user);
        }
        cmd.run_detecting_problems()?;
        Ok(())
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for GoDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        if let Some(version) = &self.version {
            Some(vec![
                crate::dependencies::debian::DebianDependency::new_with_min_version(
                    "golang-go",
                    &version.parse().unwrap(),
                ),
            ])
        } else {
            Some(vec![crate::dependencies::debian::DebianDependency::new(
                "golang-go",
            )])
        }
    }
}
