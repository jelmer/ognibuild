use crate::installer::{Installer, Error, Explanation, InstallationScope};
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

pub struct OctaveForgeResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> OctaveForgeResolver<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    fn cmd(&self, dependency: &OctavePackageDependency, scope: InstallationScope) -> Result<Vec<String>, Error> {
        match scope {
            InstallationScope::Global => Ok(vec!["octave-cli".to_string(), "--eval".to_string(), format!("pkg install -forge -global {}", dependency.package)]),
            InstallationScope::User => Ok(vec!["octave-cli".to_string(), "--eval".to_string(), format!("pkg install -forge -local {}", dependency.package)]),
            InstallationScope::Vendor => {
                Err(Error::UnsupportedScope(scope))
            }
        }
    }
}

impl<'a> Installer for OctaveForgeResolver<'a> {
    fn explain(&self, dependency: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
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
            .unwrap();
        let cmd = self.cmd(dependency, scope)?;
        log::info!("Octave: installing {}", dependency.package);
        self.session.command(cmd.iter().map(|x| x.as_str()).collect()).run_detecting_problems()?;
        Ok(())
    }
}

impl crate::dependencies::debian::IntoDebianDependency for OctavePackageDependency {
    fn try_into_debian_dependency(&self, _apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        if let Some(minimum_version) = &self.minimum_version {
            Some(vec![crate::dependencies::debian::DebianDependency::new_with_min_version(&format!("octave-{}", &self.package), &minimum_version.parse().unwrap())])
        } else {
            Some(vec![crate::dependencies::debian::DebianDependency::new(&format!("octave-{}", &self.package))])
        }
    }
}
