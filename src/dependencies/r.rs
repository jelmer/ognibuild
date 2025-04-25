use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use r_description::lossy::Relation;
use r_description::VersionConstraint;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RPackageDependency(Relation);

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

    pub fn simple(package: &str) -> Self {
        Self(
            Relation {
                name: package.to_string(),
                version: None,
            }
            .into(),
        )
    }

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

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for RPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<super::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![std::path::Path::new("/usr/lib/R/site-library")
                    .join(&self.0.name)
                    .join("DESCRIPTION")
                    .to_str()
                    .unwrap()],
                false,
                false,
            )
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .into_iter()
                .map(|name| super::debian::DebianDependency::new(&name))
                .collect(),
        )
    }
}

pub struct RResolver<'a> {
    session: &'a dyn Session,
    repos: String,
}

impl<'a> RResolver<'a> {
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

pub fn bioconductor(session: &dyn Session) -> RResolver {
    RResolver::new(session, "https://hedgehog.fhcrc.org/bioconductor")
}

pub fn cran(session: &dyn Session) -> RResolver {
    RResolver::new(session, "https://cran.r-project.org")
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingRPackage {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RPackageDependency::simple(&self.package)))
    }
}
