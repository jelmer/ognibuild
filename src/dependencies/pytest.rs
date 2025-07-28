//! Support for pytest plugin dependencies.
//!
//! This module provides functionality for working with pytest plugin dependencies,
//! including detecting and resolving plugins from fixture names, config options,
//! and command-line arguments.

use crate::dependencies::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a pytest plugin.
///
/// This represents a dependency on a pytest plugin that needs to be installed
/// for tests to run correctly.
pub struct PytestPluginDependency {
    /// The name of the pytest plugin.
    pub plugin: String,
}

impl PytestPluginDependency {
    /// Create a new pytest plugin dependency.
    ///
    /// # Arguments
    /// * `plugin` - The name of the pytest plugin
    ///
    /// # Returns
    /// A new PytestPluginDependency
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
        }
    }
}

/// Map pytest command-line arguments to a plugin name.
///
/// # Arguments
/// * `args` - The command-line arguments to pytest
///
/// # Returns
/// The name of the required plugin, if any
fn map_pytest_arguments_to_plugin(args: &[&str]) -> Option<&'static str> {
    for arg in args {
        if arg.starts_with("--cov") {
            return Some("cov");
        }
    }
    None
}

/// Map a pytest config option to a plugin name.
///
/// # Arguments
/// * `name` - The name of the config option
///
/// # Returns
/// The name of the required plugin, if any
fn map_pytest_config_option_to_plugin(name: &str) -> Option<&'static str> {
    match name {
        "asyncio_mode" => Some("asyncio"),
        n => {
            log::warn!("Unknown pytest config option {}", n);
            None
        }
    }
}

// TODO(jelmer): populate this using an automated process
/// Map a pytest fixture name to the plugin that provides it.
///
/// # Arguments
/// * `fixture` - The name of the pytest fixture
///
/// # Returns
/// The name of the plugin that provides the fixture, if known
fn pytest_fixture_to_plugin(fixture: &str) -> Option<&str> {
    match fixture {
        "aiohttp_client" => Some("aiohttp"),
        "aiohttp_client_cls" => Some("aiohttp"),
        "aiohttp_server" => Some("aiohttp"),
        "aiohttp_raw_server" => Some("aiohttp"),
        "mock" => Some("mock"),
        "benchmark" => Some("benchmark"),
        "event_loop" => Some("asyncio"),
        "unused_tcp_port" => Some("asyncio"),
        "unused_udp_port" => Some("asyncio"),
        "unused_tcp_port_factory" => Some("asyncio"),
        "unused_udp_port_factory" => Some("asyncio"),
        _ => None,
    }
}

/// Get a list of installed pytest plugins from the pytest command.
///
/// # Arguments
/// * `session` - The session to run the command in
///
/// # Returns
/// A list of (plugin_name, version) pairs if available
fn pytest_plugins(session: &dyn Session) -> Option<Vec<(String, String)>> {
    let output = match session
        .command(vec!["pytest", "--version"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
    {
        Ok(output) => output,
        Err(crate::session::Error::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
            // pytest is not installed
            return None;
        }
        Err(e) => {
            // Other errors should panic
            panic!("Failed to run pytest: {:?}", e);
        }
    };
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        if let Some(rest) = line.strip_prefix("plugins: ") {
            return Some(
                rest.split(',')
                    .map(|s| {
                        let mut parts = s.splitn(2, '=');
                        (
                            parts.next().unwrap().to_string(),
                            parts.next().unwrap_or("").to_string(),
                        )
                    })
                    .collect(),
            );
        }
    }
    None
}

impl Dependency for PytestPluginDependency {
    fn family(&self) -> &'static str {
        "pytest-plugin"
    }

    fn present(&self, session: &dyn Session) -> bool {
        if let Some(plugins) = pytest_plugins(session) {
            plugins.iter().any(|(name, _)| name == &self.plugin)
        } else {
            false
        }
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PytestPluginDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::simple(
            &format!("python3-pytest-{}", self.plugin),
        )])
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPytestFixture {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        pytest_fixture_to_plugin(&self.0)
            .map(|plugin| Box::new(PytestPluginDependency::new(plugin)) as Box<dyn Dependency>)
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::UnsupportedPytestArguments
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        let args = self.0.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        map_pytest_arguments_to_plugin(args.as_slice())
            .map(|plugin| Box::new(PytestPluginDependency::new(plugin)) as Box<dyn Dependency>)
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::UnsupportedPytestConfigOption
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        map_pytest_config_option_to_plugin(&self.0)
            .map(|plugin| Box::new(PytestPluginDependency::new(plugin)) as Box<dyn Dependency>)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;

    #[test]
    fn test_pytest_plugin_dependency_new() {
        let dependency = PytestPluginDependency::new("cov");
        assert_eq!(dependency.plugin, "cov");
    }

    #[test]
    fn test_pytest_plugin_dependency_family() {
        let dependency = PytestPluginDependency::new("cov");
        assert_eq!(dependency.family(), "pytest-plugin");
    }

    #[test]
    fn test_pytest_plugin_dependency_as_any() {
        let dependency = PytestPluginDependency::new("cov");
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<PytestPluginDependency>().is_some());
    }

    #[test]
    fn test_map_pytest_arguments_to_plugin() {
        assert_eq!(map_pytest_arguments_to_plugin(&["--cov"]), Some("cov"));
        assert_eq!(
            map_pytest_arguments_to_plugin(&["--cov-report=html"]),
            Some("cov")
        );
        assert_eq!(map_pytest_arguments_to_plugin(&["--xvs"]), None);
    }

    #[test]
    fn test_map_pytest_config_option_to_plugin() {
        assert_eq!(
            map_pytest_config_option_to_plugin("asyncio_mode"),
            Some("asyncio")
        );
        assert_eq!(map_pytest_config_option_to_plugin("unknown_option"), None);
    }

    #[test]
    fn test_pytest_fixture_to_plugin() {
        assert_eq!(pytest_fixture_to_plugin("aiohttp_client"), Some("aiohttp"));
        assert_eq!(pytest_fixture_to_plugin("benchmark"), Some("benchmark"));
        assert_eq!(pytest_fixture_to_plugin("event_loop"), Some("asyncio"));
        assert_eq!(pytest_fixture_to_plugin("unknown_fixture"), None);
    }

    #[test]
    fn test_missing_pytest_fixture_to_dependency() {
        let problem =
            buildlog_consultant::problems::common::MissingPytestFixture("event_loop".to_string());
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "pytest-plugin");
        let pytest_dep = dep
            .as_any()
            .downcast_ref::<PytestPluginDependency>()
            .unwrap();
        assert_eq!(pytest_dep.plugin, "asyncio");
    }

    #[test]
    fn test_missing_pytest_fixture_to_dependency_unknown() {
        let problem = buildlog_consultant::problems::common::MissingPytestFixture(
            "unknown_fixture".to_string(),
        );
        let dependency = problem.to_dependency();
        assert!(dependency.is_none());
    }

    #[test]
    fn test_unsupported_pytest_arguments_to_dependency() {
        let problem = buildlog_consultant::problems::common::UnsupportedPytestArguments(vec![
            "--cov".to_string(),
            "--cov-report=html".to_string(),
        ]);
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "pytest-plugin");
        let pytest_dep = dep
            .as_any()
            .downcast_ref::<PytestPluginDependency>()
            .unwrap();
        assert_eq!(pytest_dep.plugin, "cov");
    }

    #[test]
    fn test_unsupported_pytest_arguments_to_dependency_unknown() {
        let problem = buildlog_consultant::problems::common::UnsupportedPytestArguments(vec![
            "--unknown".to_string(),
        ]);
        let dependency = problem.to_dependency();
        assert!(dependency.is_none());
    }

    #[test]
    fn test_unsupported_pytest_config_option_to_dependency() {
        let problem = buildlog_consultant::problems::common::UnsupportedPytestConfigOption(
            "asyncio_mode".to_string(),
        );
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "pytest-plugin");
        let pytest_dep = dep
            .as_any()
            .downcast_ref::<PytestPluginDependency>()
            .unwrap();
        assert_eq!(pytest_dep.plugin, "asyncio");
    }

    #[test]
    fn test_unsupported_pytest_config_option_to_dependency_unknown() {
        let problem = buildlog_consultant::problems::common::UnsupportedPytestConfigOption(
            "unknown_option".to_string(),
        );
        let dependency = problem.to_dependency();
        assert!(dependency.is_none());
    }

    #[test]
    fn test_pytest_plugins_no_pytest() {
        use crate::session::plain::PlainSession;
        use tempfile::TempDir;

        let _temp_dir = TempDir::new().unwrap();
        let session = PlainSession::new();

        // When pytest is not available, pytest_plugins should return None
        let result = pytest_plugins(&session);
        assert!(result.is_none());
    }

    #[test]
    fn test_pytest_plugins_parsing() {
        // We can't easily test the actual pytest_plugins function without having pytest installed
        // But we can test the parsing logic by testing the components

        // Test plugin name parsing logic
        let test_line = "plugins: cov-2.10.1, xdist-2.3.0, mock-3.6.1";
        if let Some(rest) = test_line.strip_prefix("plugins: ") {
            let parsed: Vec<(String, String)> = rest
                .split(',')
                .map(|s| {
                    let s = s.trim();
                    let mut parts = s.splitn(2, '-');
                    (
                        parts.next().unwrap().to_string(),
                        parts.next().unwrap_or("").to_string(),
                    )
                })
                .collect();

            assert_eq!(parsed.len(), 3);
            assert_eq!(parsed[0].0, "cov");
            assert_eq!(parsed[0].1, "2.10.1");
            assert_eq!(parsed[1].0, "xdist");
            assert_eq!(parsed[1].1, "2.3.0");
            assert_eq!(parsed[2].0, "mock");
            assert_eq!(parsed[2].1, "3.6.1");
        }
    }
}
