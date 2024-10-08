use crate::analyze::AnalyzedError;
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingLatexFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        if let Some(filename) = self.0.strip_suffix(".sty") {
            Some(Box::new(LatexPackageDependency::new(filename)))
        } else {
            None
        }
    }
}

pub struct TlmgrResolver<'a> {
    session: &'a dyn Session,
    repository: String,
}

impl<'a> TlmgrResolver<'a> {
    pub fn new(session: &'a dyn Session, repository: &str) -> Self {
        Self {
            session,
            repository: repository.to_string(),
        }
    }

    fn cmd(
        &self,
        reqs: &[&LatexPackageDependency],
        scope: InstallationScope,
    ) -> Result<Vec<String>, Error> {
        let mut ret = vec![
            "tlmgr".to_string(),
            format!("--repository={}", self.repository),
            "install".to_string(),
        ];
        match scope {
            InstallationScope::User => {
                ret.push("--usermode".to_string());
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        ret.extend(reqs.iter().map(|req| req.package.clone()));
        Ok(ret)
    }
}

impl<'a> Installer for TlmgrResolver<'a> {
    fn explain(
        &self,
        dep: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        let dep = dep
            .as_any()
            .downcast_ref::<LatexPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[dep], scope)?;
        Ok(Explanation {
            message: format!("Install the LaTeX package {}", dep.package),
            command: Some(cmd),
        })
    }

    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let dep = dep
            .as_any()
            .downcast_ref::<LatexPackageDependency>()
            .ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[dep], scope)?;
        log::info!("tlmgr: running {:?}", cmd);

        match self
            .session
            .command(cmd.iter().map(|x| x.as_str()).collect())
            .run_detecting_problems()
        {
            Ok(_) => Ok(()),
            Err(AnalyzedError::Unidentified {
                lines,
                retcode,
                secondary,
            }) => {
                if lines.contains(
                    &"tlmgr: user mode not initialized, please read the documentation!".to_string(),
                ) {
                    self.session
                        .command(vec!["tlmgr", "init-usertree"])
                        .check_call()?;
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

pub fn ctan<'a>(session: &'a dyn Session) -> TlmgrResolver<'a> {
    TlmgrResolver::new(session, "ctan")
}
