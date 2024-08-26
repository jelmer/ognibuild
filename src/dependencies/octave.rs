use crate::dependency::{Installer, Error, Explanation, InstallationScope};
use crate::session::Session;
use crate::dependency::Dependency;
use serde::{Serialize, Deserialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctavePackageDependency {
    package: String,
    minimum_version: Option<String>,
}

impl OctavePackageDependency {
    pub fn new(package: &str, minimum_version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: None,
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

pub struct OctaveForgeResolver {
    session: Box<dyn Session>,
}

impl OctaveForgeResolver {
    pub fn new(session: Box<dyn Session>) -> Self {
        Self { session }
    }

    fn cmd(&self, requirement: &OctavePackageDependency, scope: InstallationScope) -> Result<Vec<String>, Error> {
        match scope {
            InstallationScope::Global => Ok(vec!["octave-cli".to_string(), "--eval".to_string(), format!("pkg install -forge -global {}", requirement.package)]),
            InstallationScope::User => Ok(vec!["octave-cli".to_string(), "--eval".to_string(), format!("pkg install -forge -local {}", requirement.package)]),
            InstallationScope::Vendor => {
                Err(Error::UnsupportedScope(scope))
            }
        }
    }
}

impl Installer for OctaveForgeResolver {
    fn explain(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
        let requirement = requirement
            .as_any()
            .downcast_ref::<OctavePackageDependency>()
            .unwrap();
        let cmd = self.cmd(requirement, scope)?;
        Ok(Explanation {
            command: Some(cmd),
            message: format!("Install Octave package {}", requirement.package),
        })
    }

    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let requirement = requirement
            .as_any()
            .downcast_ref::<OctavePackageDependency>()
            .unwrap();
        let cmd = self.cmd(requirement, scope)?;
        log::info!("Octave: installing {}", requirement.package);
        crate::analyze::run_detecting_problems(self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect(), None, false, None, None, None, None, None, None)?;
        Ok(())
    }
}

