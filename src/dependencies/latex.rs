use crate::analyze::{AnalyzedError, run_detecting_problems};
use crate::dependency::{Error, Dependency, Explanation, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LatexPackageDependency {
    pub package: String,
}

impl LatexPackageDependency {
    pub fn new(package: &str) -> Self {
        Self {
            package: package.to_string(),
        }
    }
}

impl Dependency for LatexPackageDependency {
    fn family(&self) -> &'static str {
        "latex-package"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct TlmgrResolver {
    session: Box<dyn Session>,
    user_local: bool,
    repository: String,
}

impl TlmgrResolver {
    pub fn new(session: Box<dyn Session>, user_local: bool, repository: &str) -> Self {
        Self {
            session,
            user_local,
            repository: repository.to_string(),
        }
    }

    fn cmd(&self, reqs: &[&LatexPackageDependency]) -> Vec<String> {
        let mut ret = vec![
            "tlmgr".to_string(),
            format!("--repository={}", self.repository),
            "install".to_string(),
        ];
        if self.user_local {
            ret.push("--usermode".to_string());
        }
        ret.extend(reqs.iter().map(|req| req.package.clone()));
        ret
    }
}

impl Installer for TlmgrResolver {

    fn explain(&self, dep: &dyn Dependency) -> Result<Explanation, Error> {
        let dep = dep
            .as_any()
            .downcast_ref::<LatexPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[dep]);
        Ok(Explanation {
            message: format!("Install the LaTeX package {}", dep.package),
            command: Some(cmd),
        })
    }

    fn install(&self, dep: &dyn Dependency) -> Result<(), Error> {
        let dep = dep
            .as_any()
            .downcast_ref::<LatexPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[dep]);
        log::info!("tlmgr: running {:?}", cmd);

        match run_detecting_problems(
            self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect(), None, false, None, None, None, None, None, None) {
            Ok(_) => Ok(()),
            Err(AnalyzedError::Unidentified { lines, retcode, secondary }) => {
                if lines.contains(&"tlmgr: user mode not initialized, please read the documentation!".to_string()) {
                    self.session.check_call(["tlmgr", "init-usertree"].to_vec(), None, None, None)?;
                    Ok(())
                } else {
                    Err(Error::AnalyzedError(AnalyzedError::Unidentified {
                        retcode,
                        lines,
                        secondary,
                    }))
                }
            }
            Err(e) => Err(e.into()),
        }
    }
}

pub fn ctan(session: &dyn Session, local_user: bool) -> Box<dyn Installer> {
    Box::new(TlmgrResolver::new(session, local_user, "ctan"))
}
