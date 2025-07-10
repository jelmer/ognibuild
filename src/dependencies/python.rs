//! Support for Python package dependencies.
//!
//! This module provides functionality for working with Python package dependencies,
//! including parsing and resolving PEP 508 package requirements, and integrating
//! with package managers.

use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use pep508_rs::pep440_rs;
use serde::{Deserialize, Serialize};
use std::path::Path;
#[cfg(test)]
use std::path::PathBuf;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a Python package.
///
/// This represents a dependency on a Python package from PyPI or another package
/// repository. It uses PEP 508 requirement syntax for expressing version constraints.
pub struct PythonPackageDependency(pep508_rs::Requirement);

impl From<pep508_rs::Requirement> for PythonPackageDependency {
    fn from(requirement: pep508_rs::Requirement) -> Self {
        Self(requirement)
    }
}

impl TryFrom<PythonPackageDependency> for pep508_rs::Requirement {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: PythonPackageDependency) -> Result<Self, Self::Error> {
        Ok(value.0)
    }
}

impl TryFrom<String> for PythonPackageDependency {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TryFrom<&str> for PythonPackageDependency {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use std::str::FromStr;
        let req = pep508_rs::Requirement::from_str(value)?;

        Ok(PythonPackageDependency(req))
    }
}

impl PythonPackageDependency {
    /// Get the package name.
    ///
    /// # Returns
    /// The name of the Python package
    pub fn package(&self) -> String {
        self.0.name.to_string()
    }

    /// Create a new dependency with a minimum version requirement.
    ///
    /// # Arguments
    /// * `package` - The name of the Python package
    /// * `min_version` - The minimum version required
    ///
    /// # Returns
    /// A new PythonPackageDependency
    pub fn new_with_min_version(package: &str, min_version: &str) -> Self {
        Self(pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(package.to_string()).unwrap(),
            version_or_url: Some(min_version_as_version_or_url(min_version)),
            extras: vec![],
            marker: pep508_rs::MarkerTree::TRUE,
            origin: None,
        })
    }

    /// Create a simple dependency with no version constraints.
    ///
    /// # Arguments
    /// * `package` - The name of the Python package
    ///
    /// # Returns
    /// A new PythonPackageDependency
    pub fn simple(package: &str) -> Self {
        Self(pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(package.to_string()).unwrap(),
            version_or_url: None,
            extras: vec![],
            marker: pep508_rs::MarkerTree::TRUE,
            origin: None,
        })
    }
}

/// Convert a minimum version string to a PEP 508 VersionOrUrl.
///
/// # Arguments
/// * `min_version` - The minimum version string
///
/// # Returns
/// A PEP 508 VersionOrUrl with a >= constraint
fn min_version_as_version_or_url(min_version: &str) -> pep508_rs::VersionOrUrl {
    use std::str::FromStr;
    let version_specifiers = std::iter::once(
        pep440_rs::VersionSpecifier::from_pattern(
            pep440_rs::Operator::GreaterThanEqual,
            pep440_rs::VersionPattern::verbatim(pep440_rs::Version::from_str(min_version).unwrap()),
        )
        .unwrap(),
    )
    .collect();
    pep508_rs::VersionOrUrl::VersionSpecifier(version_specifiers)
}

/// Create a PEP 508 marker for a specific Python major version.
///
/// # Arguments
/// * `major_version` - The Python major version (e.g., 2 or 3)
///
/// # Returns
/// A PEP 508 MarkerTree that requires the specified Python version
fn major_python_version_as_marker(major_version: u32) -> pep508_rs::MarkerTree {
    pep508_rs::MarkerTree::expression(pep508_rs::MarkerExpression::Version {
        key: pep508_rs::MarkerValueVersion::PythonVersion,
        specifier: pep440_rs::VersionSpecifier::equals_star_version(pep440_rs::Version::new([
            major_version as u64,
        ])),
    })
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingPythonDistribution
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        let version_or_url = self
            .minimum_version
            .as_ref()
            .map(|min_version| min_version_as_version_or_url(min_version));
        let marker = self
            .python_version
            .as_ref()
            .map(|python_major_version| {
                major_python_version_as_marker(*python_major_version as u32)
            })
            .unwrap_or(pep508_rs::MarkerTree::TRUE);

        let requirement = pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(self.distribution.clone()).unwrap(),
            version_or_url,
            extras: vec![],
            marker,
            origin: None,
        };

        Some(Box::new(PythonPackageDependency::from(requirement)))
    }
}

#[derive(Debug, Clone, Default, Copy, Serialize, Deserialize)]
/// Supported Python implementations and versions.
///
/// This enum represents the different Python implementations and versions
/// that can be used to satisfy Python package dependencies.
pub enum PythonVersion {
    /// CPython 2.x
    CPython2,
    /// CPython 3.x (default)
    #[default]
    CPython3,
    /// PyPy (Python 2 compatible)
    PyPy,
    /// PyPy (Python 3 compatible)
    PyPy3,
}

impl PythonVersion {
    /// Get the executable name for this Python version.
    ///
    /// # Returns
    /// The name of the Python executable
    pub fn executable(&self) -> &'static str {
        match self {
            PythonVersion::CPython2 => "python2",
            PythonVersion::CPython3 => "python3",
            PythonVersion::PyPy => "pypy",
            PythonVersion::PyPy3 => "pypy3",
        }
    }
}

impl Dependency for PythonPackageDependency {
    fn family(&self) -> &'static str {
        "python-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let python_version = find_python_version(self.0.marker.to_dnf()).unwrap_or_default();
        let cmd = python_version.executable();
        session
            .command(vec![
                cmd,
                "-c",
                &format!(
                    r#"import pkg_resources; pkg_resources.require("""{}""")"#,
                    self.0
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        // TODO: check in the virtualenv, if any
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for PythonPackageDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(upstream_ontologist::providers::python::remote_pypi_metadata(&self.package()))
            .ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a Python module.
///
/// This represents a dependency on a specific Python module (importable name)
/// rather than a package. Used for checking if specific imports will work.
pub struct PythonModuleDependency {
    /// The Python module name
    pub module: String,
    /// Minimum version requirement
    pub minimum_version: Option<String>,
    /// Python version (e.g., Python 2 or 3)
    pub python_version: Option<PythonVersion>,
}

impl PythonModuleDependency {
    /// Create a new Python module dependency.
    ///
    /// # Arguments
    /// * `module` - The name of the module to import
    /// * `minimum_version` - Optional minimum version requirement
    /// * `python_version` - Optional specific Python version to use
    ///
    /// # Returns
    /// A new PythonModuleDependency
    pub fn new(
        module: &str,
        minimum_version: Option<&str>,
        python_version: Option<PythonVersion>,
    ) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
            python_version,
        }
    }

    /// Create a simple Python module dependency with no version constraints.
    ///
    /// # Arguments
    /// * `module` - The name of the module to import
    ///
    /// # Returns
    /// A new PythonModuleDependency with no version constraints
    pub fn simple(module: &str) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: None,
            python_version: None,
        }
    }

    /// Get the Python executable to use for this dependency.
    ///
    /// # Returns
    /// The name of the Python executable
    fn python_executable(&self) -> &str {
        self.python_version.unwrap_or_default().executable()
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPythonModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PythonModuleDependency::new(
            &self.module,
            self.minimum_version.as_deref(),
            match self.python_version {
                Some(2) => Some(PythonVersion::CPython2),
                Some(3) => Some(PythonVersion::CPython3),
                None => None,
                _ => unimplemented!(),
            },
        )))
    }
}

impl Dependency for PythonModuleDependency {
    fn family(&self) -> &'static str {
        "python-module"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let cmd = [
            self.python_executable().to_string(),
            "-c".to_string(),
            format!(
                r#"import pkgutil; exit(0 if pkgutil.find_loader("{}") else 1)"#,
                self.module
            ),
        ];
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Resolver for Python packages using pip.
///
/// This resolver installs Python packages from PyPI using pip.
pub struct PypiResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> PypiResolver<'a> {
    /// Create a new PypiResolver with the specified session.
    ///
    /// # Arguments
    /// * `session` - The session to use for executing commands
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    /// Generate the pip command for installing the specified requirements.
    ///
    /// # Arguments
    /// * `reqs` - The Python package dependencies to install
    /// * `scope` - The installation scope (user or global)
    ///
    /// # Returns
    /// The pip command as a vector of strings
    pub fn cmd(
        &self,
        reqs: Vec<&PythonPackageDependency>,
        scope: InstallationScope,
    ) -> Result<Vec<String>, Error> {
        let mut cmd = vec!["pip".to_string(), "install".to_string()];
        match scope {
            InstallationScope::User => cmd.push("--user".to_string()),
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        cmd.extend(reqs.iter().map(|req| req.package().to_string()));
        Ok(cmd)
    }
}

impl<'a> Installer for PypiResolver<'a> {
    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let args = self.cmd(vec![req], scope)?;
        let mut cmd = self
            .session
            .command(args.iter().map(|x| x.as_str()).collect());

        match scope {
            InstallationScope::Global => {
                cmd = cmd.user("root");
            }
            InstallationScope::User => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }

        cmd.run_detecting_problems()?;
        Ok(())
    }

    fn explain(
        &self,
        requirement: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(vec![req], scope)?;
        Ok(Explanation {
            message: format!("Install pip {}", req.0.name),
            command: Some(cmd),
        })
    }
}

fn find_python_version(marker: Vec<Vec<pep508_rs::MarkerExpression>>) -> Option<PythonVersion> {
    let mut major_version = None;
    let mut implementation = None;
    for expr in marker.iter().flat_map(|x| x.iter()) {
        match expr {
            pep508_rs::MarkerExpression::Version {
                key: pep508_rs::MarkerValueVersion::PythonVersion,
                specifier,
            } => {
                let version = specifier.version();
                major_version = Some(version.release()[0] as u32);
            }
            pep508_rs::MarkerExpression::String {
                key: pep508_rs::MarkerValueString::PlatformPythonImplementation,
                operator: pep508_rs::MarkerOperator::Equal,
                value,
            } => match value.as_str() {
                "PyPy" => {
                    implementation = Some("PyPy");
                }
                _ => {}
            },
            _ => {}
        }
    }

    match (major_version, implementation) {
        (Some(2), None) => Some(PythonVersion::CPython2),
        (Some(3), None) | (None, None) => Some(PythonVersion::CPython3),
        (Some(3), Some("PyPy")) | (None, Some("PyPy")) => Some(PythonVersion::PyPy3),
        (Some(2), Some("PyPy")) => Some(PythonVersion::PyPy),
        _ => {
            log::warn!(
                "Unknown python implementation / version: {:?} {:?}",
                major_version,
                implementation
            );
            None
        }
    }
}

#[cfg(test)]
fn get_possible_python3_paths_for_python_object(mut object_path: &str) -> Vec<PathBuf> {
    let mut cpython3_regexes = vec![];
    loop {
        cpython3_regexes.extend([
            Path::new("/usr/lib/python3/dist\\-packages")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
            Path::new("/usr/lib/python3/dist\\-packages").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/lib\\-dynload").join(format!(
                "{}\\.cpython\\-.*\\.so",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
        ]);
        object_path = match object_path.rsplit_once('.') {
            Some((o, _)) => o,
            None => break,
        };
    }
    cpython3_regexes
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingSetupPyCommand
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        match self.0.as_str() {
            "test" => Some(Box::new(PythonPackageDependency::simple("setuptools"))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on Python itself.
///
/// This represents a dependency on the Python interpreter or development files.
pub struct PythonDependency {
    /// The minimum Python version required, if any.
    pub min_version: Option<String>,
}

impl PythonDependency {
    /// Create a new Python dependency with an optional minimum version.
    ///
    /// # Arguments
    /// * `min_version` - The minimum Python version required (e.g., "3.8")
    ///
    /// # Returns
    /// A new PythonDependency
    pub fn new(min_version: Option<&str>) -> Self {
        Self {
            min_version: min_version.map(|s| s.to_string()),
        }
    }

    /// Create a simple Python dependency with no version constraints.
    ///
    /// # Returns
    /// A new PythonDependency with no version constraints
    pub fn simple() -> Self {
        Self { min_version: None }
    }

    fn executable(&self) -> &str {
        match &self.min_version {
            Some(min_version) => {
                if min_version.starts_with("2") {
                    "python"
                } else {
                    "python3"
                }
            }
            None => "python3",
        }
    }
}

#[cfg(test)]
mod python_dep_tests {
    use super::*;

    #[test]
    fn test_python_dependency_new() {
        let dependency = PythonDependency::new(Some("3.6"));
        assert_eq!(dependency.min_version, Some("3.6".to_string()));
    }

    #[test]
    fn test_python_dependency_simple() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.min_version, None);
    }

    #[test]
    fn test_python_dependency_family() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.family(), "python");
    }

    #[test]
    fn test_python_dependency_executable_python3() {
        let dependency = PythonDependency::new(Some("3.6"));
        assert_eq!(dependency.executable(), "python3");
    }

    #[test]
    fn test_python_dependency_executable_python2() {
        let dependency = PythonDependency::new(Some("2.7"));
        assert_eq!(dependency.executable(), "python");
    }

    #[test]
    fn test_python_dependency_executable_default() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.executable(), "python3");
    }

    #[test]
    fn test_python_dependency_from_specs() {
        use std::str::FromStr;
        let specs = pep440_rs::VersionSpecifiers::from_str(">=3.6").unwrap();
        let dependency = PythonDependency::from(&specs);
        // The actual version might be "3.6" or "3.6.0" depending on the pep440_rs version, so we just check that it contains "3.6"
        assert!(dependency.min_version.is_some());
        assert!(dependency.min_version.as_ref().unwrap().contains("3.6"));
    }

    #[test]
    fn test_python_dependency_from_specs_no_version() {
        use std::str::FromStr;
        let specs = pep440_rs::VersionSpecifiers::from_str("==3.6").unwrap();
        let dependency = PythonDependency::from(&specs);
        assert_eq!(dependency.min_version, None);
    }
}

impl Dependency for PythonDependency {
    fn family(&self) -> &'static str {
        "python"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let cmd = match self.min_version {
            Some(ref min_version) => vec![
                self.executable().to_string(),
                "-c".to_string(),
                format!(
                    "import sys; sys.exit(0 if sys.version_info >= ({}) else 1)",
                    min_version.replace('.', ", ")
                ),
            ],
            None => vec![
                PythonVersion::default().executable().to_string(),
                "-c".to_string(),
                "import sys; sys.exit(0 if sys.version_info >= (3, 0) else 1)".to_string(),
            ],
        };
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        // Check if a virtualenv is present
        session.exists(Path::new("bin/python"))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<&pep440_rs::VersionSpecifiers> for PythonDependency {
    fn from(specs: &pep440_rs::VersionSpecifiers) -> Self {
        for specifier in specs.iter() {
            if specifier.operator() == &pep440_rs::Operator::GreaterThanEqual {
                return Self {
                    min_version: Some(specifier.version().to_string()),
                };
            }
        }
        Self { min_version: None }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths() {
        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich\\.py"),
                PathBuf::from(
                    "/usr/lib/python3\\.[0-9]+/lib\\-dynload/dulwich\\.cpython\\-.*\\.so"
                ),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("dulwich"),
        );
        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo\\.py"),
                PathBuf::from(
                    "/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo/foo\\.cpython\\-.*\\.so"
                ),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo\\.cpython\\-.*\\.so"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("cleo.foo"),
        );
    }
}
