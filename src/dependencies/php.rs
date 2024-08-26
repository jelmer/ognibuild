use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpClassDependency {
    php_class: String,
}

impl PhpClassDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PhpPackageDependency {
    pub package: String,
    pub channel: Option<String>,
    pub min_version: Option<String>,
    pub max_version: Option<String>,
}

impl PhpPackageDependency {
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
pub struct PhpExtensionDependency {
    pub extension: String,
}

impl PhpExtensionDependency {
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

impl crate::dependencies::debian::IntoDebianDependency for PhpExtensionDependency {
    fn try_into_debian_dependency(&self, _apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new(&format!("php-{}", &self.extension))])
    }
}
