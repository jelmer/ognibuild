use crate::dependency::{Dependency, Error, Explanation, Installer};
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

pub struct HackageResolver {
    session: Box<dyn Session>,
    user_local: bool,
}

impl HackageResolver {
    pub fn new(session: Box<dyn Session>, user_local: bool) -> Self {
        Self { session, user_local }
    }

    fn cmd(&self, reqs: &[&HaskellPackageDependency]) -> Vec<String> {
        let mut cmd = vec!["cabal".to_string(), "install".to_string()];

        if self.user_local {
            cmd.push("--user".to_string());
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        cmd
    }
}

impl Installer for HackageResolver {
    fn install(&self, requirement: &dyn Dependency) -> Result<(), Error> {
        let user = if self.user_local { None } else { Some("root") };
        if let Some(requirement) = requirement.as_any().downcast_ref::<HaskellPackageDependency>() {
            let cmd = self.cmd(&[requirement]);
            log::info!("Hackage: running {:?}", cmd);
            crate::analyze::run_detecting_problems(self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect() , None, false, None, user, None, None, None, None)?;
            Ok(())
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }

    fn explain(&self, requirement: &dyn Dependency) -> Result<Explanation, Error> {
        if let Some(requirement) = requirement.as_any().downcast_ref::<HaskellPackageDependency>() {
            let cmd = self.cmd(&[requirement]);
            Ok(Explanation {
                message: format!("Install Haskell package {}", requirement.package),
                command: Some(cmd),
            })
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }
}
