use crate::session::Session;

#[derive(Debug)]
pub enum Error {
    UnknownDependencyFamily,
    UnsupportedScope(InstallationScope),
    AnalyzedError(crate::analyze::AnalyzedError),
    SessionError(crate::session::Error),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::UnknownDependencyFamily => write!(f, "Unknown dependency family"),
            Error::UnsupportedScope(scope) => write!(f, "Unsupported scope: {:?}", scope),
            Error::AnalyzedError(e) => write!(f, "{}", e),
            Error::SessionError(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::analyze::AnalyzedError> for Error {
    fn from(e: crate::analyze::AnalyzedError) -> Self {
        Error::AnalyzedError(e)
    }
}

impl From<crate::session::Error> for Error {
    fn from(e: crate::session::Error) -> Self {
        Error::SessionError(e)
    }
}

/// A dependency is a component that is required by a project to build or run.
pub trait Dependency {
    fn family(&self) -> &'static str;

    /// Check whether the dependency is present in the session.
    fn present(&self, session: &dyn Session) -> bool;

    /// Check whether the dependency is present in the project.
    fn project_present(&self, session: &dyn Session) -> bool;

    fn as_any(&self) -> &dyn std::any::Any;
}

/// A resolver can take one type of dependency and resolve it into another.
pub trait Resolver {
    type Target: Dependency;
    fn resolve(&self, dep: &dyn Dependency) -> Result<Option<Self::Target>, Error>;

    /// Resolve a list of dependencies, returning only the ones that could be resolved.
    fn resolve_some(&self, deps: Vec<Box<dyn Dependency>>) -> Result<Vec<Self::Target>, Error> {
        let mut resolved = Vec::new();
        for dep in deps {
            match self.resolve(&*dep) {
                Ok(Some(dep)) => resolved.push(dep),
                Ok(None) => {}
                Err(e) => return Err(e),
            }
        }
        Ok(resolved)
    }
}

/// An explanation is a human-readable description of what to do to install a dependency.
pub struct Explanation {
    pub message: String,
    pub command: Option<Vec<String>>,
}

/// The scope of an installation.
#[derive(Debug, Clone, PartialEq, Eq, Copy)]
pub enum InstallationScope {
    /// Under /usr in the system
    Global,

    /// In the current users' home directory
    User,

    /// Vendored in the projects' source directory
    Vendor,
}

/// An installer can take a dependency and install it into the session.
pub trait Installer {
    /// Install the dependency into the session.
    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error>;

    /// Explain how to install the dependency.
    fn explain(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error>;

    fn explain_some(
        &self,
        deps: Vec<Box<dyn Dependency>>,
        scope: InstallationScope
    ) -> Result<(Vec<Explanation>, Vec<Box<dyn Dependency>>), Error> {
        let mut explanations = Vec::new();
        let mut failed = Vec::new();
        for dep in deps {
            match self.explain(&*dep, scope) {
                Ok(explanation) => explanations.push(explanation),
                Err(Error::UnknownDependencyFamily) => failed.push(dep),
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok((explanations, failed))
    }

    fn install_some(
        &self,
        deps: Vec<Box<dyn Dependency>>,
        scope: InstallationScope
    ) -> Result<(Vec<Box<dyn Dependency>>, Vec<Box<dyn Dependency>>), Error> {
        let mut installed = Vec::new();
        let mut failed = Vec::new();

        for dep in deps {
            match self.install(&*dep, scope) {
                Ok(()) => installed.push(dep),
                Err(Error::UnknownDependencyFamily) => failed.push(dep),
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok((installed, failed))
    }
}
