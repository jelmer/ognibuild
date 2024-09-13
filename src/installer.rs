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

impl Explanation {
    pub fn new(message: String, command: Option<Vec<String>>) -> Self {
        Explanation { message, command }
    }
}

impl std::fmt::Display for Explanation {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "{}", self.message)?;
        if let Some(command) = &self.command {
            write!(
                f,
                "\n\nRun the following command to install the dependency:\n\n"
            )?;
            for arg in command {
                write!(f, "{} ", arg)?;
            }
            writeln!(f)?;
        }
        Ok(())
    }
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

impl std::str::FromStr for InstallationScope {
    type Err = Error;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "global" => Ok(InstallationScope::Global),
            "user" => Ok(InstallationScope::User),
            "vendor" => Ok(InstallationScope::Vendor),
            _ => Err(Error::Other(format!("Unknown installation scope: {}", s))),
        }
    }
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
                Ok(()) => {
                    return Ok(());
                }
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }

    fn explain(
        &self,
        requirements: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        for sub in &self.0 {
            match sub.explain(requirements, scope) {
                Ok(e) => {
                    return Ok(e);
                }
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => {
                    return Err(e);
                }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }
}

pub fn installer_by_name<'a>(
    session: &'a dyn crate::session::Session,
    name: &str,
) -> Option<Box<dyn Installer + 'a>> {
    // TODO: Use more dynamic way to load installers
    match name {
        #[cfg(feature = "debian")]
        "apt" => Some(
            Box::new(crate::debian::apt::AptInstaller::from_session(session)) as Box<dyn Installer>,
        ),
        "cpan" => Some(
            Box::new(crate::dependencies::perl::CPAN::new(session, false)) as Box<dyn Installer>,
        ),
        "ctan" => Some(Box::new(crate::dependencies::latex::ctan(session)) as Box<dyn Installer>),
        "pypi" => Some(
            Box::new(crate::dependencies::python::PypiResolver::new(session)) as Box<dyn Installer>,
        ),
        "npm" => Some(
            Box::new(crate::dependencies::node::NpmResolver::new(session)) as Box<dyn Installer>,
        ),
        "go" => {
            Some(Box::new(crate::dependencies::go::GoResolver::new(session)) as Box<dyn Installer>)
        }
        "hackage" => Some(
            Box::new(crate::dependencies::haskell::HackageResolver::new(session))
                as Box<dyn Installer>,
        ),
        "cran" => Some(Box::new(crate::dependencies::r::cran(session)) as Box<dyn Installer>),
        "bioconductor" => {
            Some(Box::new(crate::dependencies::r::bioconductor(session)) as Box<dyn Installer>)
        }
        "octave-forge" => Some(
            Box::new(crate::dependencies::octave::OctaveForgeResolver::new(
                session,
            )) as Box<dyn Installer>,
        ),
        "native" => {
            Some(Box::new(StackedInstaller::new(native_installers(session))) as Box<dyn Installer>)
        }
        _ => None,
    }
}

pub fn native_installers<'a>(
    session: &'a dyn crate::session::Session,
) -> Vec<Box<dyn Installer + 'a>> {
    // TODO: Use more dynamic way to load installers
    [
        "ctan",
        "pypi",
        "npm",
        "go",
        "hackage",
        "cran",
        "bioconductor",
        "octave-forge",
    ]
    .iter()
    .map(|name| installer_by_name(session, name).unwrap())
    .collect()
}

#[cfg(feature = "debian")]
fn apt_installer<'a>(
    session: &'a dyn crate::session::Session,
    #[allow(unused_variables)] dep_server_url: Option<&url::Url>,
) -> Box<dyn Installer + 'a> {
    #[cfg(feature = "dep-server")]
    if let Some(dep_server_url) = dep_server_url {
        Box::new(
            crate::debian::dep_server::DepServerAptInstaller::from_session(session, dep_server_url),
        ) as Box<dyn Installer + 'a>
    } else {
        Box::new(crate::debian::apt::AptInstaller::from_session(session))
    }

    #[cfg(not(feature = "dep-server"))]
    {
        Box::new(crate::debian::apt::AptInstaller::from_session(session))
    }
}

/// Select installers by name.
pub fn select_installers<'a>(
    session: &'a dyn crate::session::Session,
    names: &[&str],
    dep_server_url: Option<&url::Url>,
) -> Result<Box<dyn Installer + 'a>, String> {
    let mut installers = Vec::new();
    for name in names.iter() {
        if name == &"apt" {
            #[cfg(feature = "debian")]
            installers.push(apt_installer(session, dep_server_url));
            #[cfg(not(feature = "debian"))]
            return Err("Apt installer not available".to_string());
        } else if let Some(installer) = installer_by_name(session, name) {
            installers.push(installer);
        } else {
            return Err(format!("Unknown installer: {}", name));
        }
    }
    Ok(Box::new(StackedInstaller(installers)))
}

pub fn auto_installation_scope(session: &dyn crate::session::Session) -> InstallationScope {
    let user = crate::session::get_user(session);
    // TODO(jelmer): Check VIRTUAL_ENV, and prioritize PypiResolver if
    // present?
    if user == "root" {
        log::info!("Running as root, so using global installation scope");
        InstallationScope::Global
    } else if session.is_temporary() {
        log::info!("Running in a temporary session, so using global installation scope");
        InstallationScope::Global
    } else {
        log::info!("Running as user, so using user installation scope");
        InstallationScope::User
    }
}

pub fn auto_installer<'a>(
    session: &'a dyn crate::session::Session,
    scope: InstallationScope,
    dep_server_url: Option<&url::Url>,
) -> Box<dyn Installer + 'a> {
    // if session is SchrootSession or if we're root, use apt
    let mut installers: Vec<Box<dyn Installer + 'a>> = Vec::new();
    #[cfg(feature = "debian")]
    if scope == InstallationScope::Global && crate::session::which(session, "apt-get").is_some() {
        log::info!(
            "Using global installation scope and apt-get is available, so using apt installer"
        );
        installers.push(apt_installer(session, dep_server_url));
    }
    installers.extend(native_installers(session));
    Box::new(StackedInstaller::new(installers))
}

/// Install missing dependencies.
///
/// This function takes a list of dependencies and installs them if they are not already present.
///
/// # Arguments
/// * `session` - The session to install the dependencies into.
/// * `installer` - The installer to use.
pub fn install_missing_deps(
    session: &dyn Session,
    installer: &dyn Installer,
    scope: InstallationScope,
    deps: &[&dyn Dependency],
) -> Result<(), Error> {
    if deps.is_empty() {
        return Ok(());
    }
    let mut missing = vec![];
    for dep in deps.iter() {
        if !dep.present(session) {
            missing.push(*dep)
        }
    }
    if !missing.is_empty() {
        for dep in missing.into_iter() {
            log::info!("Installing {:?}", dep);
            installer.install(dep, scope)?;
        }
    }
    Ok(())
}

/// Explain missing dependencies.
///
/// This function takes a list of dependencies and returns a list of explanations for how to
/// install them.
///
/// # Arguments
/// * `session` - The session to install the dependencies into.
/// * `installer` - The installer to use.
pub fn explain_missing_deps(
    session: &dyn Session,
    installer: &dyn Installer,
    deps: &[&dyn Dependency],
) -> Result<Vec<Explanation>, Error> {
    if deps.is_empty() {
        return Ok(vec![]);
    }
    let mut missing = vec![];
    for dep in deps.iter() {
        if !dep.present(session) {
            missing.push(*dep)
        }
    }
    if !missing.is_empty() {
        let mut explanations = vec![];
        for dep in missing.into_iter() {
            log::info!("Explaining {:?}", dep);
            explanations.push(installer.explain(dep, InstallationScope::Global)?);
        }
        Ok(explanations)
    } else {
        Ok(vec![])
    }
}
