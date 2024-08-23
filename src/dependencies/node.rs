use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct NodePackageDependency {
    package: String,
}

impl NodePackageDependency {
    pub fn new(package: &str) -> Self {
        Self {
            package: package.to_string(),
        }
    }
}

impl Dependency for NodePackageDependency {
    fn family(&self) -> &'static str {
        "npm-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // npm list -g package-name --depth=0 >/dev/null 2>&1
        session
            .command(vec!["npm", "list", "-g", &self.package, "--depth=0"])
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
pub struct NodeModuleDependency {
    module: String,
}

impl NodeModuleDependency {
    pub fn new(module: &str) -> Self {
        Self {
            module: module.to_string(),
        }
    }
}

impl Dependency for NodeModuleDependency {
    fn family(&self) -> &'static str {
        "node-module"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // node -e 'try { require.resolve("express"); process.exit(0); } catch(e) { process.exit(1); }'
        session
            .command(vec![
                "node",
                "-e",
                &format!(
                    r#"try {{ require.resolve("{}"); process.exit(0); }} catch(e) {{ process.exit(1); }}"#,
                    self.module
                ),
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
