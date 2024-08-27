use crate::session::{get_user, Session};
use debversion::Version;
use std::sync::RwLock;
use crate::dependency::Dependency;
use crate::installer::{Installer, Explanation, InstallationScope, Error as InstallerError};
use crate::dependencies::debian::{DebianDependency, TieBreaker, default_tie_breakers, IntoDebianDependency};

pub enum Error {
    Unidentified {
        retcode: i32,
        args: Vec<String>,
        lines: Vec<String>,
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
    Detailed {
        retcode: i32,
        args: Vec<String>,
        error: Option<Box<dyn buildlog_consultant::Problem>>,
    },
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

pub struct AptManager<'a> {
    pub session: &'a dyn Session,
    prefix: Vec<String>,
    searchers: RwLock<Option<Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>>>>,
}

impl<'a> AptManager<'a> {
    pub fn new(session: &'a dyn Session, prefix: Option<Vec<String>>) -> Self {
        Self {
            session,
            prefix: prefix.unwrap_or_default(),
            searchers: RwLock::new(None),
        }
    }

    pub fn from_session(session: &'a dyn Session) -> Self {
        let prefix = if get_user(session).as_str() != "root" {
            vec!["sudo".to_string()]
        } else {
            vec![]
        };
        return Self::new(session, Some(prefix));
    }

    fn run_apt(&self, args: Vec<&str>) -> Result<(), Error> {
        run_apt(
            self.session,
            args,
            self.prefix.iter().map(|s| s.as_str()).collect(),
        )
    }

    pub fn satisfy(&self, deps: Vec<&str>) -> Result<(), Error> {
        let mut args = vec!["satisfy"];
        args.extend(deps);
        self.run_apt(args)
    }

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

    pub fn get_packages_for_paths(
        &self, paths: Vec<&str>, regex: bool, case_insensitive: bool
    ) -> Result<Vec<String>, Error> {
        log::debug!("Searching for packages containing {:?}", paths);
        if self.searchers.read().unwrap().is_none() {
            *self.searchers.write().unwrap() = Some(vec![
                crate::debian::file_search::get_apt_contents_file_searcher(self.session).unwrap(),
                Box::new(crate::debian::file_search::GENERATED_FILE_SEARCHER.clone()),
            ]);
        }

        Ok(crate::debian::file_search::get_packages_for_paths(
            paths,
            self.searchers.read().unwrap().as_ref().unwrap().iter().map(|s| s.as_ref()).collect::<Vec<_>>().as_slice(),
            regex,
            case_insensitive,
        ))
    }
}

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

pub fn run_apt(session: &dyn Session, args: Vec<&str>, prefix: Vec<&str>) -> Result<(), Error> {
    let args = [prefix, vec!["apt", "-y"], args].concat();
    log::info!("apt: running {:?}", args);
    let (retcode, mut lines) = crate::session::run_with_tee(
        session,
        args.clone(),
        Some(std::path::Path::new("/")),
        Some("root"),
        None,
        None,
        None,
        None,
    )?;
    if retcode == 0 {
        return Ok(());
    }
    let (r#match, error) =
        buildlog_consultant::apt::find_apt_get_failure(lines.iter().map(|s| s.as_str()).collect());
    if let Some(error) = error {
        return Err(Error::Detailed {
            retcode,
            args: args.iter().map(|s| s.to_string()).collect(),
            error: Some(error),
        });
    }
    while lines.last().map_or(false, |line| line.trim().is_empty()) {
        lines.pop();
    }
    return Err(Error::Unidentified {
        retcode,
        args: args.iter().map(|s| s.to_string()).collect(),
        lines,
        secondary: r#match,
    });
}

fn pick_best_deb_dependency(mut dependencies: Vec<DebianDependency>, tie_breakers: &[Box<dyn TieBreaker>]) -> Option<DebianDependency> {
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

    log::info!("No tie breaker could determine a winner for dependency {:?}", dependencies);
    Some(dependencies.remove(0))
}

pub fn dependency_to_possible_deb_dependencies(apt: &AptManager, dep: &dyn Dependency) -> Vec<DebianDependency> {
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
    try_into_debian_dependency!(apt, dep, crate::dependencies::haskell::HaskellPackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JavaClassDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JDKDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JREDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::java::JDKFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::BinaryDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::pytest::PytestPluginDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::VcsControlDirectoryAccessDependency);
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
    try_into_debian_dependency!(apt, dep, crate::dependencies::CertificateAuthorityDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::autoconf::AutoconfMacroDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::LibtoolDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::BoostComponentDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::KF5ComponentDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::IntrospectionTypelibDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::node::NodePackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::node::NodeModuleDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::octave::OctavePackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::perl::PerlPreDeclaredDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::perl::PerlModuleDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::perl::PerlFileDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::php::PhpClassDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::php::PhpExtensionDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::python::PythonModuleDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::r::RPackageDependency);
    try_into_debian_dependency!(apt, dep, crate::dependencies::vague::VagueDependency);

    candidates
}

pub fn dependency_to_deb_dependency(apt: &AptManager, dep: &dyn Dependency, tie_breakers: &[Box<dyn TieBreaker>]) -> Result<Option<DebianDependency>, InstallerError> {
    let candidates = dependency_to_possible_deb_dependencies(apt, dep);

    if candidates.is_empty() {
        return Ok(None);
    }

    Ok(pick_best_deb_dependency(candidates, tie_breakers))
}

pub struct AptInstaller<'a> {
    apt: AptManager<'a>,
    tie_breakers: Vec<Box<dyn TieBreaker>>,
}

impl<'a> AptInstaller<'a> {
    pub fn new(apt: AptManager<'a>) -> Self {
        let tie_breakers = default_tie_breakers(apt.session);
        Self { apt, tie_breakers }
    }

    pub fn new_with_tie_breakers(apt: AptManager<'a>, tie_breakers: Vec<Box<dyn TieBreaker>>) -> Self {
        Self { apt, tie_breakers }
    }

    /// Create a new AptInstaller from a session
    pub fn from_session(session: &'a dyn Session) -> Self {
        Self::new(AptManager::from_session(session))
    }
}


impl<'a> Installer for AptInstaller<'a> {
    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), InstallerError> {
        match scope {
            InstallationScope::User => {
                return Err(InstallerError::UnsupportedScope(scope));
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(InstallerError::UnsupportedScope(scope));
            }
        }
        if dep.present(self.apt.session) {
            return Ok(());
        }

        let apt_deb = if let Some(apt_deb) = dependency_to_deb_dependency(&self.apt, dep, self.tie_breakers.as_slice())? {
            apt_deb
        } else {
            return Err(InstallerError::UnknownDependencyFamily);
        };

        match self.apt.satisfy(vec![apt_deb.relation_string().as_str()]) {
            Ok(_) => {},
            Err(e) => { return Err(InstallerError::Other(e.to_string())); }
        }
        Ok(())
    }

    fn explain(&self, dep: &dyn Dependency, _scope: InstallationScope) -> Result<Explanation, InstallerError> {
        let apt_deb = if let Some(apt_deb) = dependency_to_deb_dependency(&self.apt, dep, self.tie_breakers.as_slice())? {
            apt_deb
        } else {
            return Err(InstallerError::UnknownDependencyFamily);
        };

        let apt_deb_str = apt_deb.relation_string();
        let cmd = self.apt.satisfy_command(vec![apt_deb_str.as_str()]);
        Ok(Explanation {
            message: format!("Install {}", apt_deb.package_names().iter().map(|x| x.as_str()).collect::<Vec<_>>().join(", ")),
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
        assert_eq!(pick_best_deb_dependency(vec![dep1.clone()], tie_breakers.as_mut_slice()), Some(dep1.clone()));

        // No dependencies
        assert_eq!(pick_best_deb_dependency(vec![], tie_breakers.as_mut_slice()), None);

        // Multiple dependencies
        assert_eq!(pick_best_deb_dependency(vec![dep1.clone(), dep2.clone()], tie_breakers.as_mut_slice()), Some(dep1.clone()));
    }
}
