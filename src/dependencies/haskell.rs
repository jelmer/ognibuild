use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a Haskell package
pub struct HaskellPackageDependency {
    package: String,
    specs: Option<Vec<String>>,
}

impl HaskellPackageDependency {
    /// Creates a new HaskellPackageDependency
    pub fn new(package: &str, specs: Option<Vec<&str>>) -> Self {
        Self {
            package: package.to_string(),
            specs: specs.map(|v| v.iter().map(|s| s.to_string()).collect()),
        }
    }

    /// Creates a new HaskellPackageDependency with no specs
    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            specs: None,
        }
    }
}

impl std::str::FromStr for HaskellPackageDependency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(2, ' ');
        let package = parts.next().ok_or("missing package name")?.to_string();
        let specs = parts.next().map(|s| s.split(' ').collect());
        Ok(Self::new(&package, specs))
    }
}

fn ghc_pkg_list(session: &dyn Session) -> Vec<(String, String)> {
    let output = session
        .command(vec!["ghc-pkg", "list"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    output
        .lines()
        .filter_map(|line| {
            if let Some((name, version)) =
                line.strip_prefix("    ").and_then(|s| s.rsplit_once('-'))
            {
                Some((name.to_string(), version.to_string()))
            } else {
                None
            }
        })
        .collect()
}

impl Dependency for HaskellPackageDependency {
    fn family(&self) -> &'static str {
        "haskell-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // TODO: Check version
        ghc_pkg_list(session)
            .iter()
            .any(|(name, _version)| name == &self.package)
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for HaskellPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<super::debian::DebianDependency>> {
        let path = format!(
            "/var/lib/ghc/package\\.conf\\.d/{}\\-.*\\.conf",
            regex::escape(&self.package)
        );

        let names = apt
            .get_packages_for_paths(vec![path.as_str()], true, false)
            .unwrap();
        if names.is_empty() {
            None
        } else {
            Some(
                names
                    .into_iter()
                    .map(|name| super::debian::DebianDependency::new(&name))
                    .collect(),
            )
        }
    }
}

/// A resolver for Haskell packages using the `cabal` command
pub struct HackageResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> HackageResolver<'a> {
    /// Creates a new HackageResolver
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    fn cmd(
        &self,
        reqs: &[&HaskellPackageDependency],
        scope: InstallationScope,
    ) -> Result<Vec<String>, Error> {
        let mut cmd = vec!["cabal".to_string(), "install".to_string()];

        match scope {
            InstallationScope::User => {
                cmd.push("--user".to_string());
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        Ok(cmd)
    }
}

impl<'a> Installer for HackageResolver<'a> {
    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let requirement = requirement
            .as_any()
            .downcast_ref::<HaskellPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let user = if scope != InstallationScope::Global {
            None
        } else {
            Some("root")
        };
        let cmd = self.cmd(&[requirement], scope)?;
        log::info!("Hackage: running {:?}", cmd);
        let mut cmd = self
            .session
            .command(cmd.iter().map(|x| x.as_str()).collect());
        if let Some(user) = user {
            cmd = cmd.user(user);
        }
        cmd.run_detecting_problems()?;
        Ok(())
    }

    fn explain(
        &self,
        requirement: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        if let Some(requirement) = requirement
            .as_any()
            .downcast_ref::<HaskellPackageDependency>()
        {
            let cmd = self.cmd(&[requirement], scope)?;
            Ok(Explanation {
                message: format!("Install Haskell package {}", requirement.package),
                command: Some(cmd),
            })
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingHaskellDependencies
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        let d: HaskellPackageDependency = self.0[0].parse().unwrap();
        Some(Box::new(d))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;
    use std::str::FromStr;

    #[test]
    fn test_haskell_package_dependency_new() {
        let dependency = HaskellPackageDependency::new("parsec", Some(vec![">=3.1.11"]));
        assert_eq!(dependency.package, "parsec");
        assert_eq!(dependency.specs, Some(vec![">=3.1.11".to_string()]));
    }

    #[test]
    fn test_haskell_package_dependency_simple() {
        let dependency = HaskellPackageDependency::simple("parsec");
        assert_eq!(dependency.package, "parsec");
        assert_eq!(dependency.specs, None);
    }

    #[test]
    fn test_haskell_package_dependency_family() {
        let dependency = HaskellPackageDependency::simple("parsec");
        assert_eq!(dependency.family(), "haskell-package");
    }

    #[test]
    fn test_haskell_package_dependency_as_any() {
        let dependency = HaskellPackageDependency::simple("parsec");
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<HaskellPackageDependency>().is_some());
    }

    #[test]
    fn test_haskell_package_dependency_from_str() {
        let dependency = HaskellPackageDependency::from_str("parsec >=3.1.11").unwrap();
        assert_eq!(dependency.package, "parsec");
        assert_eq!(dependency.specs, Some(vec![">=3.1.11".to_string()]));
    }

    #[test]
    fn test_missing_haskell_dependencies_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingHaskellDependencies(vec![
            "parsec".to_string(),
        ]);
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "haskell-package");
        let haskell_dep = dep
            .as_any()
            .downcast_ref::<HaskellPackageDependency>()
            .unwrap();
        assert_eq!(haskell_dep.package, "parsec");
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for HaskellPackageDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(upstream_ontologist::providers::haskell::remote_hackage_data(&self.package))
            .ok()
    }
}
