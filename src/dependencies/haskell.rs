use crate::dependency::{Dependency, Error, Explanation, Installer, InstallationScope};
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HaskellPackageDependency {
    package: String,
    specs: Option<Vec<String>>,
}

impl HaskellPackageDependency {
    pub fn new(package: &str, specs: Option<Vec<&str>>) -> Self {
        Self {
            package: package.to_string(),
            specs: specs.map(|v| v.iter().map(|s| s.to_string()).collect()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            specs: None,
        }
    }
}

fn ghc_pkg_list(session: &dyn Session) -> Vec<(String, String)> {
    let output = session
        .command(vec!["ghc-pkg", "list"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    let output = String::from_utf8(output.stdout).unwrap();
    output
        .lines()
        .filter_map(|line| {
            if let Some((name, version)) =
                line.strip_prefix("    ").and_then(|s| s.rsplit_once('-'))
            {
                Some((name.to_string(), version.to_string()))
            } else {
                None
            }
        })
        .collect()
}

impl Dependency for HaskellPackageDependency {
    fn family(&self) -> &'static str {
        "haskell-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // TODO: Check version
        ghc_pkg_list(session)
            .iter()
            .any(|(name, _version)| name == &self.package)
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::dependencies::debian::IntoDebianDependency for HaskellPackageDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> Option<Vec<super::debian::DebianDependency>> {
        let path = format!("/var/lib/ghc/package\\.conf\\.d/{}\\-.*\\.conf", regex::escape(&self.package));

        let names = apt.get_packages_for_paths(vec![path.as_str()], true, false).unwrap();
        if names.is_empty() {
            None
        } else {
            Some(names.into_iter().map(|name| super::debian::DebianDependency::new(&name)).collect())
        }
    }
}

pub struct HackageResolver {
    session: Box<dyn Session>,
}

impl HackageResolver {
    pub fn new(session: Box<dyn Session>) -> Self {
        Self { session }
    }

    fn cmd(&self, reqs: &[&HaskellPackageDependency], scope: InstallationScope) -> Result<Vec<String>, Error> {
        let mut cmd = vec!["cabal".to_string(), "install".to_string()];

        match scope {
            InstallationScope::User => {
                cmd.push("--user".to_string());
            }
            InstallationScope::Global => {},
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        Ok(cmd)
    }
}

impl Installer for HackageResolver {
    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let user = if scope != InstallationScope::Global { None } else { Some("root") };
        if let Some(requirement) = requirement.as_any().downcast_ref::<HaskellPackageDependency>() {
            let cmd = self.cmd(&[requirement], scope)?;
            log::info!("Hackage: running {:?}", cmd);
            crate::analyze::run_detecting_problems(self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect() , None, false, None, user, None, None, None, None)?;
            Ok(())
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }

    fn explain(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
        if let Some(requirement) = requirement.as_any().downcast_ref::<HaskellPackageDependency>() {
            let cmd = self.cmd(&[requirement], scope)?;
            Ok(Explanation {
                message: format!("Install Haskell package {}", requirement.package),
                command: Some(cmd),
            })
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }
}
