//! Support for PHP dependencies.
//!
//! This module provides functionality for working with PHP dependencies,
//! including PHP classes, packages via Composer, and PHP extensions.

use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a PHP class.
///
/// This represents a dependency on a specific PHP class that needs to be available
/// for the code to function properly.
pub struct PhpClassDependency {
    /// The name of the PHP class
    pub php_class: String,
}

impl PhpClassDependency {
    /// Create a new PHP class dependency.
    ///
    /// # Arguments
    /// * `php_class` - The name of the PHP class
    ///
    /// # Returns
    /// A new PhpClassDependency
    pub fn new(php_class: &str) -> Self {
        Self {
            php_class: php_class.to_string(),
        }
    }
}

impl Dependency for PhpClassDependency {
    fn family(&self) -> &'static str {
        "php-class"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["php", "-r", &format!("new {}", self.php_class)])
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
    use crate::buildlog::ToDependency;
    use std::any::Any;

    #[test]
    fn test_php_class_dependency_new() {
        let dependency = PhpClassDependency::new("SimplePie");
        assert_eq!(dependency.php_class, "SimplePie");
    }

    #[test]
    fn test_php_class_dependency_family() {
        let dependency = PhpClassDependency::new("SimplePie");
        assert_eq!(dependency.family(), "php-class");
    }

    #[test]
    fn test_php_class_dependency_as_any() {
        let dependency = PhpClassDependency::new("SimplePie");
        let any_dep: &dyn Any = dependency.as_any();
        assert!(any_dep.downcast_ref::<PhpClassDependency>().is_some());
    }

    #[test]
    fn test_missing_php_class_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingPhpClass {
            php_class: "SimplePie".to_string(),
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "php-class");
        let php_dep = dep.as_any().downcast_ref::<PhpClassDependency>().unwrap();
        assert_eq!(php_dep.php_class, "SimplePie");
    }

    #[test]
    fn test_php_package_dependency_simple() {
        let dependency = PhpPackageDependency::simple("symfony/console");
        assert_eq!(dependency.package, "symfony/console");
        assert_eq!(dependency.channel, None);
        assert_eq!(dependency.min_version, None);
        assert_eq!(dependency.max_version, None);
    }

    #[test]
    fn test_php_package_dependency_new() {
        let dependency = PhpPackageDependency::new(
            "symfony/console",
            Some("packagist"),
            Some("4.0.0"),
            Some("5.0.0"),
        );
        assert_eq!(dependency.package, "symfony/console");
        assert_eq!(dependency.channel, Some("packagist".to_string()));
        assert_eq!(dependency.min_version, Some("4.0.0".to_string()));
        assert_eq!(dependency.max_version, Some("5.0.0".to_string()));
    }

    #[test]
    fn test_php_package_dependency_family() {
        let dependency = PhpPackageDependency::simple("symfony/console");
        assert_eq!(dependency.family(), "php-package");
    }

    #[test]
    fn test_php_extension_dependency_new() {
        let dependency = PhpExtensionDependency::new("curl");
        assert_eq!(dependency.extension, "curl");
    }

    #[test]
    fn test_php_extension_dependency_family() {
        let dependency = PhpExtensionDependency::new("curl");
        assert_eq!(dependency.family(), "php-extension");
    }

    #[test]
    fn test_missing_php_extension_to_dependency() {
        let problem =
            buildlog_consultant::problems::common::MissingPHPExtension("curl".to_string());
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "php-extension");
        let php_dep = dep
            .as_any()
            .downcast_ref::<PhpExtensionDependency>()
            .unwrap();
        assert_eq!(php_dep.extension, "curl");
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPhpClass {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PhpClassDependency::new(&self.php_class)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a PHP package installed via Composer.
///
/// This represents a dependency on a PHP package that can be installed
/// through Composer, with optional version constraints and channel specification.
pub struct PhpPackageDependency {
    /// The name of the PHP package.
    pub package: String,
    /// The channel to install from (e.g., "pear").
    pub channel: Option<String>,
    /// The minimum version required.
    pub min_version: Option<String>,
    /// The maximum version allowed.
    pub max_version: Option<String>,
}

impl PhpPackageDependency {
    /// Create a new PHP package dependency with version constraints.
    ///
    /// # Arguments
    /// * `package` - The name of the PHP package
    /// * `channel` - Optional channel to install from
    /// * `min_version` - Optional minimum version constraint
    /// * `max_version` - Optional maximum version constraint
    ///
    /// # Returns
    /// A new PhpPackageDependency
    pub fn new(
        package: &str,
        channel: Option<&str>,
        min_version: Option<&str>,
        max_version: Option<&str>,
    ) -> Self {
        Self {
            package: package.to_string(),
            channel: channel.map(|s| s.to_string()),
            min_version: min_version.map(|s| s.to_string()),
            max_version: max_version.map(|s| s.to_string()),
        }
    }

    /// Create a simple PHP package dependency with no version constraints.
    ///
    /// # Arguments
    /// * `package` - The name of the PHP package
    ///
    /// # Returns
    /// A new PhpPackageDependency with no version constraints
    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            channel: None,
            min_version: None,
            max_version: None,
        }
    }
}

impl Dependency for PhpPackageDependency {
    fn family(&self) -> &'static str {
        "php-package"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        // Run `composer show` and check the output
        let output = session
            .command(vec!["composer", "show", "--format=json"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();

        let packages: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let packages = packages["installed"].as_array().unwrap();
        packages.iter().any(|package| {
            package["name"] == self.package
                && (self.min_version.is_none()
                    || package["version"]
                        .as_str()
                        .unwrap()
                        .parse::<semver::Version>()
                        .unwrap()
                        >= self
                            .min_version
                            .as_ref()
                            .unwrap()
                            .parse::<semver::Version>()
                            .unwrap())
                && (self.max_version.is_none()
                    || package["version"]
                        .as_str()
                        .unwrap()
                        .parse::<semver::Version>()
                        .unwrap()
                        <= self
                            .max_version
                            .as_ref()
                            .unwrap()
                            .parse::<semver::Version>()
                            .unwrap())
        })
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a PHP extension.
///
/// This represents a dependency on a PHP extension that needs to be installed
/// and enabled for the code to function properly.
pub struct PhpExtensionDependency {
    /// The name of the PHP extension.
    pub extension: String,
}

impl PhpExtensionDependency {
    /// Create a new PHP extension dependency.
    ///
    /// # Arguments
    /// * `extension` - The name of the PHP extension
    ///
    /// # Returns
    /// A new PhpExtensionDependency
    pub fn new(extension: &str) -> Self {
        Self {
            extension: extension.to_string(),
        }
    }
}

impl Dependency for PhpExtensionDependency {
    fn family(&self) -> &'static str {
        "php-extension"
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn present(&self, session: &dyn Session) -> bool {
        // Grep the output of php -m
        let output = session
            .command(vec!["php", "-m"])
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap()
            .stdout;
        String::from_utf8(output)
            .unwrap()
            .lines()
            .any(|line| line == self.extension)
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPHPExtension {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PhpExtensionDependency::new(&self.0)))
    }
}
