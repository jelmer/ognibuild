//! APT package management functionality for Debian packages.
//!
//! This module provides interfaces for installing and managing packages
//! using the APT package manager, as well as utilities for converting
//! generic dependencies to Debian package dependencies.

use crate::dependencies::debian::{
    default_tie_breakers, DebianDependency, IntoDebianDependency, TieBreaker,
};
use crate::dependency::Dependency;
use crate::installer::{Error as InstallerError, Explanation, InstallationScope, Installer};
use crate::session::{get_user, Session};
use debversion::Version;
use std::sync::RwLock;

/// Errors that can occur when using APT.
pub enum Error {
    /// An unidentified error occurred while running apt.
    Unidentified {
        /// The return code from apt.
        retcode: i32,
        /// The command-line arguments passed to apt.
        args: Vec<String>,
        /// The output lines from apt.
        lines: Vec<String>,
        /// Secondary match information from buildlog-consultant.
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
    /// A detailed error occurred while running apt.
    Detailed {
        /// The return code from apt.
        retcode: i32,
        /// The command-line arguments passed to apt.
        args: Vec<String>,
        /// The error from buildlog-consultant.
        error: Option<Box<dyn buildlog_consultant::Problem>>,
    },
    /// An error occurred in the session.
    Session(crate::session::Error),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unidentified {
                retcode,
                args,
                lines,
                secondary: _,
            } => {
                write!(
                    f,
                    "Unidentified error: apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    lines.join("\n")
                )
            }
            Error::Detailed {
                retcode,
                args,
                error,
            } => {
                write!(
                    f,
                    "Detailed error: apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    error.as_ref().map_or("".to_string(), |e| e.to_string())
                )
            }
            Error::Session(error) => write!(f, "{:?}", error),
        }
    }
}

impl From<crate::session::Error> for Error {
    fn from(error: crate::session::Error) -> Self {
        Error::Session(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unidentified {
                retcode,
                args,
                lines,
                secondary: _,
            } => {
                write!(
                    f,
                    "apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    lines.join("\n")
                )
            }
            Error::Detailed {
                retcode,
                args,
                error,
            } => {
                write!(
                    f,
                    "apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    error.as_ref().map_or("".to_string(), |e| e.to_string())
                )
            }
            Error::Session(error) => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for Error {}

/// Manager for APT operations.
///
/// This struct provides methods for interacting with APT,
/// including dependency resolution and package installation.
pub struct AptManager<'a> {
    /// The session to run APT commands in.
    session: &'a dyn Session,
    /// Command prefix (e.g., "sudo") for APT commands.
    prefix: Vec<String>,
    /// File searchers for finding packages containing files.
    searchers: RwLock<Option<Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>>>>,
}

/// Entry for APT satisfy command.
///
/// Represents either a required package or a package conflict.
pub enum SatisfyEntry {
    /// A required package dependency.
    Required(String),
    /// A package that conflicts with installation.
    Conflict(String),
}

impl<'a> AptManager<'a> {
    /// Get the session associated with this APT manager.
    ///
    /// # Returns
    /// Reference to the session
    pub fn session(&self) -> &'a dyn Session {
        self.session
    }

    /// Create a new APT manager.
    ///
    /// # Arguments
    /// * `session` - Session to run APT commands in
    /// * `prefix` - Optional command prefix (e.g., "sudo")
    ///
    /// # Returns
    /// A new AptManager instance
    pub fn new(session: &'a dyn Session, prefix: Option<Vec<String>>) -> Self {
        Self {
            session,
            prefix: prefix.unwrap_or_default(),
            searchers: RwLock::new(None),
        }
    }

    /// Set file searchers for finding packages containing files.
    ///
    /// # Arguments
    /// * `searchers` - List of file searchers to use
    pub fn set_searchers(
        &self,
        searchers: Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>>,
    ) {
        *self.searchers.write().unwrap() = Some(searchers);
    }

    /// Create a new APT manager from a session with appropriate sudo prefix.
    ///
    /// Automatically adds "sudo" to the command prefix if the session user is not root.
    ///
    /// # Arguments
    /// * `session` - Session to run APT commands in
    ///
    /// # Returns
    /// A new AptManager instance
    pub fn from_session(session: &'a dyn Session) -> Self {
        let prefix = if get_user(session).as_str() != "root" {
            vec!["sudo".to_string()]
        } else {
            vec![]
        };
        return Self::new(session, Some(prefix));
    }

    /// Run an APT command with the configured prefix.
    ///
    /// # Arguments
    /// * `args` - Arguments to pass to APT
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn run_apt(&self, args: Vec<&str>) -> Result<(), Error> {
        run_apt(
            self.session,
            args,
            self.prefix.iter().map(|s| s.as_str()).collect(),
        )
    }

    /// Satisfy package dependencies using APT.
    ///
    /// # Arguments
    /// * `deps` - List of dependencies to satisfy (required or conflicts)
    ///
    /// # Returns
    /// Ok on success, Error if dependencies cannot be satisfied
    pub fn satisfy(&self, deps: Vec<SatisfyEntry>) -> Result<(), Error> {
        let mut args = vec!["satisfy".to_string()];
        args.extend(deps.iter().map(|dep| match dep {
            SatisfyEntry::Required(s) => s.clone(),
            SatisfyEntry::Conflict(s) => format!("Conflict: {}", s),
        }));
        self.run_apt(args.iter().map(|s| s.as_str()).collect())
    }

    /// Generate a satisfy command for the given dependencies.
    ///
    /// # Arguments
    /// * `deps` - List of dependency strings
    ///
    /// # Returns
    /// Command-line arguments for satisfying the dependencies
    pub fn satisfy_command<'b>(&'b self, deps: Vec<&'b str>) -> Vec<&'b str> {
        let mut args = self
            .prefix
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>();
        args.push("apt");
        args.push("satisfy");
        args.extend(deps);
        args
    }

    /// Find packages that contain the specified paths.
    ///
    /// # Arguments
    /// * `paths` - List of file paths to search for
    /// * `regex` - Whether to treat paths as regular expressions
    /// * `case_insensitive` - Whether to ignore case in path matching
    ///
    /// # Returns
    /// List of package names that contain the paths
    pub fn get_packages_for_paths(
        &self,
        paths: Vec<&str>,
        regex: bool,
        case_insensitive: bool,
    ) -> Result<Vec<String>, Error> {
        if regex {
            log::debug!("Searching for packages containing regexes {:?}", paths);
        } else {
            log::debug!("Searching for packages containing {:?}", paths);
        }
        if self.searchers.read().unwrap().is_none() {
            *self.searchers.write().unwrap() = Some(vec![
                crate::debian::file_search::get_apt_contents_file_searcher(self.session).unwrap(),
                Box::new(crate::debian::file_search::GENERATED_FILE_SEARCHER.clone()),
            ]);
        }

        Ok(crate::debian::file_search::get_packages_for_paths(
            paths,
            self.searchers
                .read()
                .unwrap()
                .as_ref()
                .unwrap()
                .iter()
                .map(|s| s.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
            regex,
            case_insensitive,
        ))
    }
}

/// Find simple Debian dependencies for the given paths.
///
/// # Arguments
/// * `apt_mgr` - APT manager to use
/// * `paths` - List of file paths to search for
/// * `regex` - Whether to treat paths as regular expressions
/// * `case_insensitive` - Whether to ignore case in path matching
///
/// # Returns
/// List of Debian dependencies for packages containing the paths
pub fn find_deps_simple(
    apt_mgr: &AptManager,
    paths: Vec<&str>,
    regex: bool,
    case_insensitive: bool,
) -> Result<Vec<DebianDependency>, Error> {
    let packages = apt_mgr.get_packages_for_paths(paths, regex, case_insensitive)?;
    Ok(packages
        .iter()
        .map(|package| DebianDependency::simple(package))
        .collect())
}

/// Find Debian dependencies with minimum version for the given paths.
///
/// # Arguments
/// * `apt_mgr` - APT manager to use
/// * `paths` - List of file paths to search for
/// * `regex` - Whether to treat paths as regular expressions
/// * `minimum_version` - Minimum version requirement
/// * `case_insensitive` - Whether to ignore case in path matching
///
/// # Returns
/// List of versioned Debian dependencies for packages containing the paths
pub fn find_deps_with_min_version(
    apt_mgr: &AptManager,
    paths: Vec<&str>,
    regex: bool,
    minimum_version: &Version,
    case_insensitive: bool,
) -> Result<Vec<DebianDependency>, Error> {
    let packages = apt_mgr.get_packages_for_paths(paths, regex, case_insensitive)?;
    Ok(packages
        .iter()
        .map(|package| DebianDependency::new_with_min_version(package, minimum_version))
        .collect())
}

/// Run an APT command with the given prefix.
///
/// # Arguments
/// * `session` - Session to run the command in
/// * `args` - Arguments to pass to APT
/// * `prefix` - Command prefix (e.g., "sudo")
///
/// # Returns
/// Ok on success, Error otherwise
pub fn run_apt(session: &dyn Session, args: Vec<&str>, prefix: Vec<&str>) -> Result<(), Error> {
    let args = [prefix, vec!["apt", "-y"], args].concat();
    log::info!("apt: running {:?}", args);
    let (status, mut lines) = session
        .command(args.clone())
        .cwd(std::path::Path::new("/"))
        .user("root")
        .run_with_tee()?;
    if status.success() {
        return Ok(());
    }
    let (r#match, error) =
        buildlog_consultant::apt::find_apt_get_failure(lines.iter().map(|s| s.as_str()).collect());
    if let Some(error) = error {
        return Err(Error::Detailed {
            retcode: status.code().unwrap_or(1),
            args: args.iter().map(|s| s.to_string()).collect(),
            error: Some(error),
        });
    }
    while lines.last().map_or(false, |line| line.trim().is_empty()) {
        lines.pop();
    }
    return Err(Error::Unidentified {
        retcode: status.code().unwrap_or(1),
        args: args.iter().map(|s| s.to_string()).collect(),
        lines,
        secondary: r#match,
    });
}

/// Pick the best Debian dependency from a list of candidates.
///
/// Uses tie breakers to determine the best dependency when multiple candidates exist.
///
/// # Arguments
/// * `dependencies` - List of Debian dependency candidates
/// * `tie_breakers` - List of tie breakers to use
///
/// # Returns
/// The best dependency, or None if no candidates are available
fn pick_best_deb_dependency(
    mut dependencies: Vec<DebianDependency>,
    tie_breakers: &[Box<dyn TieBreaker>],
) -> Option<DebianDependency> {
    if dependencies.is_empty() {
        return None;
    }

    if dependencies.len() == 1 {
        return Some(dependencies.remove(0));
    }

    log::warn!("Multiple candidates for dependency {:?}", dependencies);

    for tie_breaker in tie_breakers {
        let winner = tie_breaker.break_tie(dependencies.iter().collect::<Vec<_>>().as_slice());
        if let Some(winner) = winner {
            return Some(winner.clone());
        }
    }

    log::info!(
        "No tie breaker could determine a winner for dependency {:?}",
        dependencies
    );
    Some(dependencies.remove(0))
}

/// Convert a generic dependency to possible Debian dependencies.
///
/// Attempts to convert a dependency to Debian-specific dependencies using
/// various conversion strategies for different dependency types.
///
/// # Arguments
/// * `apt` - APT manager to use for lookups
/// * `dep` - The generic dependency to convert
///
/// # Returns
/// List of possible Debian dependencies
pub fn dependency_to_possible_deb_dependencies(
    apt: &AptManager,
    dep: &dyn Dependency,
) -> Vec<DebianDependency> {
    let mut candidates = vec![];
    macro_rules! try_into_debian_dependency {
        ($apt:expr, $dep:expr, $type:ty) => {
            if let Some(dep) = $dep.as_any().downcast_ref::<$type>() {
                if let Some(alts) = dep.try_into_debian_dependency($apt) {
                    candidates.extend(alts);
                }
            }
        };
    }

    // TODO: More idiomatic way to do this?
    try_into_debian_dependency!(apt, dep, crate::dependencies::go::GoPackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::go::GoDependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::haskell::HaskellPackageDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JavaClassDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JDKDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JREDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JDKFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::BinaryDependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::pytest::PytestPluginDependency
    );
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::VcsControlDirectoryAccessDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::CargoCrateDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::PkgConfigDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::PathDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::CHeaderDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::JavaScriptRuntimeDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::ValaPackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::RubyGemDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::DhAddonDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::LibraryDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::StaticLibraryDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::RubyFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::xml::XmlEntityDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::SprocketsFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::CMakeFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::MavenArtifactDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::GnomeCommonDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::QtModuleDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::QTDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::X11Dependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::CertificateAuthorityDependency
    );
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::autoconf::AutoconfMacroDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::LibtoolDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::BoostComponentDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::KF5ComponentDependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::IntrospectionTypelibDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::node::NodePackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::node::NodeModuleDependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::octave::OctavePackageDependency
    );
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::perl::PerlPreDeclaredDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::perl::PerlModuleDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::perl::PerlFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::php::PhpClassDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::php::PhpExtensionDependency);
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::python::PythonModuleDependency
    );
    try_into_debian_dependency!(
        apt,
        dep,
        crate::dependencies::python::PythonPackageDependency
    );
    try_into_debian_dependency!(apt, dep, crate::dependencies::python::PythonDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::r::RPackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::vague::VagueDependency);

    candidates
}

/// Convert a generic dependency to the best Debian dependency.
///
/// First finds all possible Debian dependencies for the given generic dependency,
/// then uses tie breakers to pick the best one if multiple candidates exist.
///
/// # Arguments
/// * `apt` - APT manager to use for lookups
/// * `dep` - The generic dependency to convert
/// * `tie_breakers` - List of tie breakers to use
///
/// # Returns
/// The best Debian dependency, or None if no candidates are available
pub fn dependency_to_deb_dependency(
    apt: &AptManager,
    dep: &dyn Dependency,
    tie_breakers: &[Box<dyn TieBreaker>],
) -> Result<Option<DebianDependency>, InstallerError> {
    let mut candidates = dependency_to_possible_deb_dependencies(apt, dep);

    if candidates.is_empty() {
        log::debug!("No Debian dependency candidates for dependency {:?}", dep);
        Ok(None)
    } else if candidates.len() == 1 {
        let deb_dep = candidates.remove(0);
        log::debug!(
            "Only one Debian dependency candidate for dependency {:?}: {:?}",
            dep,
            deb_dep
        );
        Ok(Some(deb_dep))
    } else {
        Ok(pick_best_deb_dependency(candidates, tie_breakers))
    }
}

/// Installer that uses APT to install dependencies.
///
/// This installer converts generic dependencies to Debian package dependencies
/// and installs them using APT.
pub struct AptInstaller<'a> {
    /// The APT manager to use for package operations
    apt: AptManager<'a>,
    /// Tie breakers for selecting among multiple dependency candidates
    tie_breakers: Vec<Box<dyn TieBreaker>>,
}

impl<'a> AptInstaller<'a> {
    /// Create a new APT installer with default tie breakers.
    ///
    /// # Arguments
    /// * `apt` - APT manager to use
    ///
    /// # Returns
    /// A new AptInstaller instance
    pub fn new(apt: AptManager<'a>) -> Self {
        let tie_breakers = default_tie_breakers(apt.session);
        Self { apt, tie_breakers }
    }

    /// Create a new APT installer with custom tie breakers.
    ///
    /// # Arguments
    /// * `apt` - APT manager to use
    /// * `tie_breakers` - Custom tie breakers for selecting among dependencies
    ///
    /// # Returns
    /// A new AptInstaller instance
    pub fn new_with_tie_breakers(
        apt: AptManager<'a>,
        tie_breakers: Vec<Box<dyn TieBreaker>>,
    ) -> Self {
        Self { apt, tie_breakers }
    }

    /// Create a new APT installer from a session.
    ///
    /// Creates an APT manager with appropriate sudo prefix if needed.
    ///
    /// # Arguments
    /// * `session` - Session to run APT commands in
    ///
    /// # Returns
    /// A new AptInstaller instance
    pub fn from_session(session: &'a dyn Session) -> Self {
        Self::new(AptManager::from_session(session))
    }
}

/// Implementation of the Installer trait for AptInstaller.
impl<'a> Installer for AptInstaller<'a> {
    /// Install a dependency using APT.
    ///
    /// Only supports the Global installation scope.
    ///
    /// # Arguments
    /// * `dep` - Dependency to install
    /// * `scope` - Installation scope
    ///
    /// # Returns
    /// Ok on success, Error if installation fails
    fn install(
        &self,
        dep: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<(), InstallerError> {
        match scope {
            InstallationScope::User => {
                return Err(InstallerError::UnsupportedScope(scope));
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(InstallerError::UnsupportedScope(scope));
            }
        }

        let apt_deb = if let Some(apt_deb) =
            dependency_to_deb_dependency(&self.apt, dep, self.tie_breakers.as_slice())?
        {
            apt_deb
        } else {
            return Err(InstallerError::UnknownDependencyFamily);
        };

        match self
            .apt
            .satisfy(vec![SatisfyEntry::Required(apt_deb.relation_string())])
        {
            Ok(_) => {}
            Err(e) => {
                return Err(InstallerError::Other(e.to_string()));
            }
        }
        Ok(())
    }

    /// Explain how to install a dependency using APT.
    ///
    /// # Arguments
    /// * `dep` - Dependency to explain
    /// * `_scope` - Installation scope (ignored)
    ///
    /// # Returns
    /// An explanation with message and optional command
    fn explain(
        &self,
        dep: &dyn Dependency,
        _scope: InstallationScope,
    ) -> Result<Explanation, InstallerError> {
        let apt_deb = if let Some(apt_deb) =
            dependency_to_deb_dependency(&self.apt, dep, self.tie_breakers.as_slice())?
        {
            apt_deb
        } else {
            return Err(InstallerError::UnknownDependencyFamily);
        };

        let apt_deb_str = apt_deb.relation_string();
        let cmd = self.apt.satisfy_command(vec![apt_deb_str.as_str()]);
        Ok(Explanation {
            message: format!(
                "Install {}",
                apt_deb
                    .package_names()
                    .iter()
                    .map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            command: Some(cmd.iter().map(|s| s.to_string()).collect()),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_pick_best_deb_dependency() {
        struct DummyTieBreaker;
        impl crate::dependencies::debian::TieBreaker for DummyTieBreaker {
            fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency> {
                reqs.iter().next().cloned()
            }
        }

        let mut tie_breakers = vec![Box::new(DummyTieBreaker) as Box<dyn TieBreaker>];

        let dep1 = DebianDependency::new("libssl-dev");
        let dep2 = DebianDependency::new("libssl1.1-dev");

        // Single dependency
        assert_eq!(
            pick_best_deb_dependency(vec![dep1.clone()], tie_breakers.as_mut_slice()),
            Some(dep1.clone())
        );

        // No dependencies
        assert_eq!(
            pick_best_deb_dependency(vec![], tie_breakers.as_mut_slice()),
            None
        );

        // Multiple dependencies
        assert_eq!(
            pick_best_deb_dependency(
                vec![dep1.clone(), dep2.clone()],
                tie_breakers.as_mut_slice()
            ),
            Some(dep1.clone())
        );
    }
}
