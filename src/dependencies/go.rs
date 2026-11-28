use crate::dependency::{Dependency, DependencyFamily};
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};

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
    fn test_go_version() {
        let v = |s: &str| s.parse::<GoVersion>().unwrap();
        assert_eq!(
            v("1.24.4"),
            GoVersion {
                major: 1,
                minor: 24,
                patch: Some(4),
                prerelease: None
            }
        );
        assert_eq!(
            v("1.21rc1"),
            GoVersion {
                major: 1,
                minor: 21,
                patch: None,
                prerelease: Some((GoPrerelease::Rc, 1))
            }
        );
        assert!(v("1.24.4") > v("1.21"));
        assert!(v("1.20.14") < v("1.21"));
        assert!(v("1.9") < v("1.21"));
        assert!(v("1.21") < v("1.21beta2"));
        assert!(v("1.20rc1") < v("1.20"));
        assert_eq!(v("1.20"), v("1.20.0"));
        assert!(v("1.21beta2") < v("1.21rc1"));
        assert!(v("1.21rc1") < v("1.21.0"));
        assert!(v("1.21.0") < v("1.21.1"));
        assert!(v("1.22") > v("1.21rc1"));
        for bad in ["", "1", "1.x", "1.21.0rc1", "1.21foo1", "1.21.0.1", "devel"] {
            assert_eq!(
                bad.parse::<GoVersion>(),
                Err(format!("invalid Go version {:?}", bad))
            );
        }
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

inventory::submit! { DependencyFamily::new::<GoPackageDependency>("go-package") }

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for GoPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        use std::path::Path;

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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum GoPrerelease {
    Alpha,
    Beta,
    Rc,
}

/// A Go version, ordered as Go does: "1.21" < "1.21rc1" < "1.21.0".
#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
struct GoVersion {
    major: u64,
    minor: u64,
    patch: Option<u64>,
    prerelease: Option<(GoPrerelease, u64)>,
}

impl std::str::FromStr for GoVersion {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let invalid = || format!("invalid Go version {:?}", s);
        let number = |n: &str| n.parse::<u64>().map_err(|_| invalid());
        let (release, pre) =
            s.split_at(s.find(|c: char| c.is_ascii_alphabetic()).unwrap_or(s.len()));
        let mut parts = release.split('.');
        let major = number(parts.next().ok_or_else(invalid)?)?;
        let minor = number(parts.next().ok_or_else(invalid)?)?;
        let patch = parts.next().map(number).transpose()?;
        if parts.next().is_some() || (patch.is_some() && !pre.is_empty()) {
            return Err(invalid());
        }
        // Before Go 1.21, "1.20" is the release "1.20.0".
        let patch = match patch {
            None if pre.is_empty() && (major, minor) < (1, 21) => Some(0),
            patch => patch,
        };
        let prerelease = if pre.is_empty() {
            None
        } else if let Some(n) = pre.strip_prefix("rc") {
            Some((GoPrerelease::Rc, number(n)?))
        } else if let Some(n) = pre.strip_prefix("beta") {
            Some((GoPrerelease::Beta, number(n)?))
        } else if let Some(n) = pre.strip_prefix("alpha") {
            Some((GoPrerelease::Alpha, number(n)?))
        } else {
            return Err(invalid());
        };
        Ok(GoVersion {
            major,
            minor,
            patch,
            prerelease,
        })
    }
}

impl Dependency for GoDependency {
    fn family(&self) -> &'static str {
        "go"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let output = session
            .command(vec!["go", "version"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        if !output.status.success() {
            return false;
        }
        let Some(wanted) = &self.version else {
            return true;
        };
        // e.g. "go version go1.24.4 linux/amd64"
        let output = String::from_utf8_lossy(&output.stdout);
        let version = output
            .split_whitespace()
            .nth(2)
            .and_then(|v| v.strip_prefix("go"));
        let found = match version {
            Some(found) => found.parse::<GoVersion>(),
            None => Err(format!("unexpected output from go version: {:?}", output)),
        };
        // Either way, treat it as no usable Go, so that Go gets installed.
        match (found, wanted.parse::<GoVersion>()) {
            (Ok(found), Ok(wanted)) => found >= wanted,
            (Err(e), _) | (_, Err(e)) => {
                log::warn!("{}", e);
                false
            }
        }
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        unimplemented!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

inventory::submit! { DependencyFamily::new::<GoDependency>("go") }

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
            command: Some(self.cmd(&[req])),
        })
    }

    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<GoPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[req]);
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
