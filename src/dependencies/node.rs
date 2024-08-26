use crate::dependencies::BinaryDependency;
use crate::dependency::{Installer, Error, Explanation, Dependency, InstallationScope};
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

    fn command_package(command: &str) -> Option<&str> {
        match command {
            "del-cli" => Some("del-cli"),
            "husky" => Some("husky"),
            "cross-env" => Some("cross-env"),
            "xo" => Some("xo"),
            "standard" => Some("standard"),
            "jshint" => Some("jshint"),
            "if-node-version" => Some("if-node-version"),
            "babel-cli" => Some("babel"),
            "c8" => Some("c8"),
            "prettier-standard" => Some("prettier-standard"),
            _ => None,
        }
    }

pub struct NpmResolver {
    session: Box<dyn Session>,
}

impl NpmResolver {
    pub fn new(session: Box<dyn Session>) -> Self {
        Self { session }
    }

    fn cmd(&self, reqs: &[&NodePackageDependency], scope: InstallationScope) -> Result<Vec<String>, Error> {
        let mut cmd = vec!["npm".to_string(), "install".to_string()];
        match scope {
            InstallationScope::Global => cmd.push("-g".to_string()),
            InstallationScope::User => {},
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        Ok(cmd)
    }

}

impl From<NodeModuleDependency> for NodePackageDependency {
    fn from(dep: NodeModuleDependency) -> Self {
        let parts: Vec<&str> = dep.module.split('/').collect();
        Self {
            // TODO: Is this legit?
            package: if parts[0].starts_with('@') {
                parts[..2].join("/")
            } else {
                parts[0].to_string()
            }
        }
    }
}

fn to_node_package_req(requirement: &dyn Dependency) -> Option<NodePackageDependency> {
    if let Some(requirement) = requirement.as_any().downcast_ref::<NodeModuleDependency>() {
        Some(requirement.clone().into())
    } else if let Some(requirement) = requirement.as_any().downcast_ref::<NodePackageDependency>() {
        Some(requirement.clone())
    } else if let Some(requirement) = requirement.as_any().downcast_ref::<BinaryDependency>() {
        command_package(&requirement.binary_name).map(NodePackageDependency::new)
    } else {
        None
    }
}

impl Installer for NpmResolver {
    fn explain(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
        let requirement = to_node_package_req(requirement)
            .ok_or(Error::UnknownDependencyFamily)?;

        Ok(Explanation {
            message: format!("install node package {}", requirement.package),
            command: Some(self.cmd(&[&requirement], scope)?),
        })
    }

    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let requirement = to_node_package_req(requirement)
            .ok_or(Error::UnknownDependencyFamily)?;

        let cmd = &self.cmd(&[&requirement], scope)?;
        crate::analyze::run_detecting_problems(
            self.session.as_ref(),
            cmd.iter().map(|s| s.as_str()).collect(),
            None,
            false,
            None,
            match scope {
                InstallationScope::Global => Some("root"),
                InstallationScope::User => None,
                InstallationScope::Vendor => None,
            },
            None,
            None,
            None,
            None
        )?;

        Ok(())
    }
}
