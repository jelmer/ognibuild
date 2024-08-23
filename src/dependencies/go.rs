use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoPackageDependency {
    package: String,
    version: Option<String>,
}

impl GoPackageDependency {
    pub fn new(package: &str, version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            version: version.map(|s| s.to_string()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            version: None,
        }
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
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GoDependency {
    version: Option<String>,
}

impl GoDependency {
    pub fn new(version: Option<&str>) -> Self {
        Self {
            version: version.map(|s| s.to_string()),
        }
    }
}

impl Dependency for GoDependency {
    fn family(&self) -> &'static str {
        "go"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["go".to_string(), "version".to_string()];
        if let Some(version) = &self.version {
            cmd.push(format!(">={}", version));
        }
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        unimplemented!()
    }
}
