use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// OctavePackageDependency represents a dependency on an Octave package.
pub struct OctavePackageDependency {
    /// The name of the Octave package
    pub package: String,
    /// Optional minimum version requirement
    pub minimum_version: Option<String>,
}

impl OctavePackageDependency {
    /// Creates a new OctavePackageDependency with a specified minimum version.
    pub fn new(package: &str, minimum_version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    /// Creates a new OctavePackageDependency with no minimum version.
    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: None,
        }
    }
}

impl std::str::FromStr for OctavePackageDependency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        if let Some((_, name, min_version)) = lazy_regex::regex_captures!("(.*) \\(>= (.*)\\)", s) {
            Ok(Self::new(name, Some(min_version)))
        } else if !s.contains(" ") {
            Ok(Self::simple(s))
        } else {
            Err(format!("Failed to parse Octave package dependency: {}", s))
        }
    }
}

impl Dependency for OctavePackageDependency {
    fn family(&self) -> &'static str {
        "octave-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec![
                "octave",
                "--eval",
                &format!("pkg load {}", self.package),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[test]
    fn test_octave_package_dependency_new() {
        let dependency = OctavePackageDependency::new("signal", Some("1.0.0"));
        assert_eq!(dependency.package, "signal");
        assert_eq!(dependency.minimum_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_octave_package_dependency_simple() {
        let dependency = OctavePackageDependency::simple("signal");
        assert_eq!(dependency.package, "signal");
        assert_eq!(dependency.minimum_version, None);
    }

    #[test]
    fn test_octave_package_dependency_family() {
        let dependency = OctavePackageDependency::simple("signal");
        assert_eq!(dependency.family(), "octave-package");
    }

    #[test]
    fn test_octave_package_dependency_as_any() {
        let dependency = OctavePackageDependency::simple("signal");
        let any_dep: &dyn Any = dependency.as_any();
        assert!(any_dep.downcast_ref::<OctavePackageDependency>().is_some());
    }

    #[test]
    fn test_octave_package_dependency_from_str_simple() {
        let dependency: OctavePackageDependency = "signal".parse().unwrap();
        assert_eq!(dependency.package, "signal");
        assert_eq!(dependency.minimum_version, None);
    }

    #[test]
    fn test_octave_package_dependency_from_str_with_version() {
        let dependency: OctavePackageDependency = "signal (>= 1.0.0)".parse().unwrap();
        assert_eq!(dependency.package, "signal");
        assert_eq!(dependency.minimum_version, Some("1.0.0".to_string()));
    }

    #[test]
    fn test_octave_package_dependency_from_str_invalid() {
        let result: Result<OctavePackageDependency, _> = "signal with bad format".parse();
        assert!(result.is_err());
    }
}

/// OctaveForgeResolver is an installer for Octave packages using the Octave Forge repository.
pub struct OctaveForgeResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> OctaveForgeResolver<'a> {
    /// Creates a new OctaveForgeResolver.
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    fn cmd(
        &self,
        dependency: &OctavePackageDependency,
        scope: InstallationScope,
    ) -> Result<Vec<String>, Error> {
        match scope {
            InstallationScope::Global => Ok(vec![
                "octave-cli".to_string(),
                "--eval".to_string(),
                format!("pkg install -forge -global {}", dependency.package),
            ]),
            InstallationScope::User => Ok(vec![
                "octave-cli".to_string(),
                "--eval".to_string(),
                format!("pkg install -forge -local {}", dependency.package),
            ]),
            InstallationScope::Vendor => Err(Error::UnsupportedScope(scope)),
        }
    }
}

impl<'a> Installer for OctaveForgeResolver<'a> {
    fn explain(
        &self,
        dependency: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        let dependency = dependency
            .as_any()
            .downcast_ref::<OctavePackageDependency>()
            .unwrap();
        let cmd = self.cmd(dependency, scope)?;
        Ok(Explanation {
            command: Some(cmd),
            message: format!("Install Octave package {}", dependency.package),
        })
    }

    fn install(&self, dependency: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let dependency = dependency
            .as_any()
            .downcast_ref::<OctavePackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(dependency, scope)?;
        log::info!("Octave: installing {}", dependency.package);
        self.session
            .command(cmd.iter().map(|x| x.as_str()).collect())
            .run_detecting_problems()?;
        Ok(())
    }
}
