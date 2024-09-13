use crate::dependencies::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PytestPluginDependency {
    pub plugin: String,
}

impl PytestPluginDependency {
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
        }
    }
}

fn map_pytest_arguments_to_plugin(args: &[&str]) -> Option<&'static str> {
    for arg in args {
        if arg.starts_with("--cov") {
            return Some("cov");
        }
    }
    None
}

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

fn pytest_plugins(session: &dyn Session) -> Option<Vec<(String, String)>> {
    let output = session
        .command(vec!["pytest", "--version"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
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
