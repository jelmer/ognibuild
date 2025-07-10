//! Support for R package dependencies.
//!
//! This module provides functionality for working with R package dependencies,
//! including parsing and resolving package requirements from CRAN and Bioconductor.

use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use r_description::lossy::Relation;
use r_description::VersionConstraint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on an R package.
///
/// This represents a dependency on an R package from CRAN, Bioconductor, or another repository.
pub struct RPackageDependency(pub Relation);

impl From<RPackageDependency> for Relation {
    fn from(dep: RPackageDependency) -> Self {
        dep.0
    }
}

impl From<Relation> for RPackageDependency {
    fn from(rel: Relation) -> Self {
        Self(rel)
    }
}

impl From<r_description::lossless::Relation> for RPackageDependency {
    fn from(rel: r_description::lossless::Relation) -> Self {
        Self(rel.into())
    }
}

impl RPackageDependency {
    /// Create a new R package dependency with an optional minimum version.
    ///
    /// # Arguments
    /// * `package` - The name of the R package
    /// * `minimum_version` - Optional minimum version requirement
    ///
    /// # Returns
    /// A new RPackageDependency
    pub fn new(package: &str, minimum_version: Option<&str>) -> Self {
        if let Some(minimum_version) = minimum_version {
            Self(
                Relation {
                    name: package.to_string(),
                    version: Some((
                        VersionConstraint::GreaterThanEqual,
                        minimum_version.parse().unwrap(),
                    )),
                }
                .into(),
            )
        } else {
            Self(
                Relation {
                    name: package.to_string(),
                    version: None,
                }
                .into(),
            )
        }
    }

    /// Create a simple R package dependency with no version constraints.
    ///
    /// # Arguments
    /// * `package` - The name of the R package
    ///
    /// # Returns
    /// A new RPackageDependency with no version constraints
    pub fn simple(package: &str) -> Self {
        Self(
            Relation {
                name: package.to_string(),
                version: None,
            }
            .into(),
        )
    }

    /// Create an R package dependency from a string representation.
    ///
    /// # Arguments
    /// * `s` - String representation of the dependency (e.g., "dplyr (>= 1.0.0)")
    ///
    /// # Returns
    /// A new RPackageDependency parsed from the string
    pub fn from_str(s: &str) -> Self {
        if let Some((_, name, min_version)) = lazy_regex::regex_captures!("(.*) \\(>= (.*)\\)", s) {
            Self::new(name, Some(min_version))
        } else if !s.contains(" ") {
            Self::simple(s)
        } else {
            panic!("Invalid R package dependency: {}", s);
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

/// A resolver for R package dependencies.
///
/// This resolver installs R packages from repositories like CRAN and Bioconductor.
pub struct RResolver<'a> {
    /// The session to use for command execution
    pub session: &'a dyn Session,
    /// The repository URL for R packages
    pub repos: String,
}

impl<'a> RResolver<'a> {
    /// Create a new RResolver with the specified session and repository.
    ///
    /// # Arguments
    /// * `session` - The session to use for executing commands
    /// * `repos` - The R repository URL
    ///
    /// # Returns
    /// A new RResolver
    pub fn new(session: &'a dyn Session, repos: &str) -> Self {
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
            format!("install.packages('{}', repos='{})'", req.0.name, self.repos),
        ]
    }
}

impl<'a> Installer for RResolver<'a> {
    /// Install the dependency into the session.
    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = dep
            .as_any()
            .downcast_ref::<RPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let args = self.cmd(req);
        log::info!("RResolver({:?}): running {:?}", self.repos, args);
        let mut cmd = self
            .session
            .command(args.iter().map(|x| x.as_str()).collect());
        match scope {
            InstallationScope::User => {}
            InstallationScope::Global => {
                cmd = cmd.user("root");
            }
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }

        cmd.run_detecting_problems()?;

        Ok(())
    }

    /// Explain how to install the dependency.
    fn explain(
        &self,
        dep: &dyn Dependency,
        _scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        if let Some(req) = dep.as_any().downcast_ref::<RPackageDependency>() {
            Ok(Explanation {
                message: format!("Install R package {}", req.0.name),
                command: Some(self.cmd(req)),
            })
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }
}

/// Create an RResolver for Bioconductor packages.
///
/// # Arguments
/// * `session` - The session to use for executing commands
///
/// # Returns
/// An RResolver configured for Bioconductor
pub fn bioconductor(session: &dyn Session) -> RResolver {
    RResolver::new(session, "https://hedgehog.fhcrc.org/bioconductor")
}

/// Create an RResolver for CRAN packages.
///
/// # Arguments
/// * `session` - The session to use for executing commands
///
/// # Returns
/// An RResolver configured for CRAN
pub fn cran(session: &dyn Session) -> RResolver {
    RResolver::new(session, "https://cran.r-project.org")
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingRPackage {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RPackageDependency::simple(&self.package)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;

    #[test]
    fn test_r_package_dependency_new() {
        let dependency = RPackageDependency::new("dplyr", Some("1.0.0"));
        assert_eq!(dependency.0.name, "dplyr");
        assert!(dependency.0.version.is_some());
        let (constraint, version) = dependency.0.version.unwrap();
        assert_eq!(constraint, VersionConstraint::GreaterThanEqual);
        assert_eq!(format!("{}", version), "1.0.0");
    }

    #[test]
    fn test_r_package_dependency_simple() {
        let dependency = RPackageDependency::simple("dplyr");
        assert_eq!(dependency.0.name, "dplyr");
        assert!(dependency.0.version.is_none());
    }

    #[test]
    fn test_r_package_dependency_from_str() {
        let dependency = RPackageDependency::from_str("dplyr (>= 1.0.0)");
        assert_eq!(dependency.0.name, "dplyr");
        assert!(dependency.0.version.is_some());
        let (constraint, version) = dependency.0.version.unwrap();
        assert_eq!(constraint, VersionConstraint::GreaterThanEqual);
        assert_eq!(format!("{}", version), "1.0.0");

        let dependency = RPackageDependency::from_str("dplyr");
        assert_eq!(dependency.0.name, "dplyr");
        assert!(dependency.0.version.is_none());
    }

    #[test]
    fn test_r_package_dependency_family() {
        let dependency = RPackageDependency::simple("dplyr");
        assert_eq!(dependency.family(), "r-package");
    }

    #[test]
    fn test_r_package_dependency_as_any() {
        let dependency = RPackageDependency::simple("dplyr");
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<RPackageDependency>().is_some());
    }

    #[test]
    fn test_missing_r_package_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingRPackage {
            package: "dplyr".to_string(),
            minimum_version: None,
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "r-package");
        let r_dep = dep.as_any().downcast_ref::<RPackageDependency>().unwrap();
        assert_eq!(r_dep.0.name, "dplyr");
    }

    #[test]
    fn test_r_resolver_new() {
        let session = crate::session::plain::PlainSession::new();
        let resolver = RResolver::new(&session, "https://cran.r-project.org");
        assert_eq!(resolver.repos, "https://cran.r-project.org");
    }

    #[test]
    fn test_r_resolver_cmd() {
        let session = crate::session::plain::PlainSession::new();
        let resolver = RResolver::new(&session, "https://cran.r-project.org");
        let dependency = RPackageDependency::simple("dplyr");
        let cmd = resolver.cmd(&dependency);
        assert_eq!(
            cmd,
            vec![
                "R",
                "-e",
                "install.packages('dplyr', repos='https://cran.r-project.org)'",
            ]
        );
    }

    #[test]
    fn test_bioconductor() {
        let session = crate::session::plain::PlainSession::new();
        let resolver = bioconductor(&session);
        assert_eq!(resolver.repos, "https://hedgehog.fhcrc.org/bioconductor");
    }

    #[test]
    fn test_cran() {
        let session = crate::session::plain::PlainSession::new();
        let resolver = cran(&session);
        assert_eq!(resolver.repos, "https://cran.r-project.org");
    }
}
