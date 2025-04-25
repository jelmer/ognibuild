use crate::analyze::AnalyzedError;
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a LaTeX package
pub struct LatexPackageDependency {
    /// The name of the LaTeX package
    pub package: String,
}

impl LatexPackageDependency {
    /// Creates a new `LatexPackageDependency` instance
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;
    use std::any::Any;

    #[test]
    fn test_latex_package_dependency_new() {
        let dependency = LatexPackageDependency::new("graphicx");
        assert_eq!(dependency.package, "graphicx");
    }

    #[test]
    fn test_latex_package_dependency_family() {
        let dependency = LatexPackageDependency::new("graphicx");
        assert_eq!(dependency.family(), "latex-package");
    }

    #[test]
    fn test_latex_package_dependency_as_any() {
        let dependency = LatexPackageDependency::new("graphicx");
        let any_dep: &dyn Any = dependency.as_any();
        assert!(any_dep.downcast_ref::<LatexPackageDependency>().is_some());
    }

    #[test]
    fn test_missing_latex_file_to_dependency() {
        let problem =
            buildlog_consultant::problems::common::MissingLatexFile("graphicx.sty".to_string());
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "latex-package");
        let latex_dep = dep
            .as_any()
            .downcast_ref::<LatexPackageDependency>()
            .unwrap();
        assert_eq!(latex_dep.package, "graphicx");
    }

    #[test]
    fn test_missing_latex_file_non_sty_to_dependency() {
        // Non .sty files should return None
        let problem =
            buildlog_consultant::problems::common::MissingLatexFile("graphicx.cls".to_string());
        let dependency = problem.to_dependency();
        assert!(dependency.is_none());
    }
}

/// A resolver for LaTeX package dependencies using tlmgr
pub struct TlmgrResolver<'a> {
    session: &'a dyn Session,
    repository: String,
}

impl<'a> TlmgrResolver<'a> {
    /// Creates a new `TlmgrResolver` instance
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

/// Creates a new `TlmgrResolver` instance for the CTAN repository
pub fn ctan<'a>(session: &'a dyn Session) -> TlmgrResolver<'a> {
    TlmgrResolver::new(session, "ctan")
}
