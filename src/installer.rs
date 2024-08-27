use crate::dependency::Dependency;
use crate::session::Session;

#[derive(Debug)]
pub enum Error {
    UnknownDependencyFamily,
    UnsupportedScope(InstallationScope),
    AnalyzedError(crate::analyze::AnalyzedError),
    SessionError(crate::session::Error),
    Other(String),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::UnknownDependencyFamily => write!(f, "Unknown dependency family"),
            Error::UnsupportedScope(scope) => write!(f, "Unsupported scope: {:?}", scope),
            Error::AnalyzedError(e) => write!(f, "{}", e),
            Error::SessionError(e) => write!(f, "{}", e),
            Error::Other(s) => write!(f, "{}", s),
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
    fn explain(&self, dep: &dyn Dependency, scope: InstallationScope)
        -> Result<Explanation, Error>;

    fn explain_some(
        &self,
        deps: Vec<Box<dyn Dependency>>,
        scope: InstallationScope,
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
        scope: InstallationScope,
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

/// A null installer does nothing.
pub struct NullInstaller;

impl NullInstaller {
    pub fn new() -> Self {
        NullInstaller
    }
}

impl Default for NullInstaller {
    fn default() -> Self {
        NullInstaller::new()
    }
}

impl Installer for NullInstaller {
    fn install(&self, _dep: &dyn Dependency, _scope: InstallationScope) -> Result<(), Error> {
        Err(Error::UnknownDependencyFamily)
    }

    fn explain(
        &self,
        _dep: &dyn Dependency,
        _scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        Err(Error::UnknownDependencyFamily)
    }
}

pub struct StackedInstaller<'a>(pub Vec<Box<dyn Installer + 'a>>);

impl<'a> StackedInstaller<'a> {
    pub fn new(resolvers: Vec<Box<dyn Installer + 'a>>) -> Self {
        Self(resolvers)
    }
}

impl<'a> Installer for StackedInstaller<'a> {
    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        for sub in &self.0 {
            match sub.install(requirement, scope) {
                Ok(()) => { return Ok(()); },
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => { return Err(e); }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }

    fn explain(&self, requirements: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
        for sub in &self.0 {
            match sub.explain(requirements, scope) {
                Ok(e) => { return Ok(e); },
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => { return Err(e); }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }
}

pub fn installer_by_name<'a>(session: &'a dyn crate::session::Session, name: &str) -> Option<Box<dyn Installer + 'a>> {
    // TODO: Use more dynamic way to load installers
    match name {
        "apt" => Some(Box::new(crate::debian::apt::AptInstaller::from_session(session)) as Box<dyn Installer>),
        "cpan" => Some(Box::new(crate::dependencies::perl::CPAN::new(session, false)) as Box<dyn Installer>),
        "ctan" => Some(Box::new(crate::dependencies::latex::ctan(session)) as Box<dyn Installer>),
        "pypi" => Some(Box::new(crate::dependencies::python::PypiResolver::new(session)) as Box<dyn Installer>),
        "npm" => Some(Box::new(crate::dependencies::node::NpmResolver::new(session)) as Box<dyn Installer>),
        "go" => Some(Box::new(crate::dependencies::go::GoResolver::new(session)) as Box<dyn Installer>),
        "hackage" => Some(Box::new(crate::dependencies::haskell::HackageResolver::new(session)) as Box<dyn Installer>),
        "cran" => Some(Box::new(crate::dependencies::r::cran(session)) as Box<dyn Installer>),
        "bioconductor" => Some(Box::new(crate::dependencies::r::bioconductor(session)) as Box<dyn Installer>),
        "octave-forge" => Some(Box::new(crate::dependencies::octave::OctaveForgeResolver::new(session)) as Box<dyn Installer>),
        "native" => Some(Box::new(StackedInstaller::new(native_installers(session))) as Box<dyn Installer>),
        _ => None
    }
}

pub fn native_installers<'a>(session: &'a dyn crate::session::Session) -> Vec<Box<dyn Installer + 'a>> {
    // TODO: Use more dynamic way to load installers
    ["ctan", "pypi", "npm", "go", "hackage", "cran", "bioconductor", "octave-forge"]
        .iter()
        .map(|name| installer_by_name(session, name).unwrap())
        .collect()
}

pub fn select_installers<'a>(
    session: &'a dyn crate::session::Session,
    names: Vec<String>,
) -> Vec<Box<dyn Installer + 'a>> {
    let mut installers = Vec::new();
    for name in names {
        if let Some(installer) = installer_by_name(session, &name) {
            installers.push(installer);
        }
    }
    installers
}

pub fn auto_installer<'a>(
    session: &'a dyn crate::session::Session,
    explain: bool,
    system_wide : Option<bool>,
    dep_server_url: Option<url::Url>,
) -> Box<dyn Installer + 'a> {
    // if session is SchrootSession or if we're root, use apt
    let mut installers: Vec<Box<dyn Installer + 'a>> = Vec::new();
    let has_apt = crate::session::which(session, "apt-get").is_some();
    let system_wide = if let Some(system_wide) = system_wide {
        system_wide
    } else {
        let user = crate::session::get_user(session);
        // TODO(jelmer): Check VIRTUAL_ENV, and prioritize PypiResolver if
        // present?
        if has_apt && (session.is_temporary() || user == "root" || explain) {
            true
        } else {
            false
        }
    };
    if system_wide {
        if has_apt {
            if let Some(dep_server_url) = dep_server_url {
                installers.push(
                    Box::new(crate::debian::dep_server::DepServerAptInstaller::from_session(session, dep_server_url)) as Box<dyn Installer + 'a>
                );
            } else {
                installers.push(Box::new(crate::debian::apt::AptInstaller::from_session(session)));
            }
        }
    }
    installers.extend(native_installers(session));
    Box::new(StackedInstaller::new(installers))
}
