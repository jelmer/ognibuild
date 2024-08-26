use crate::session::{get_user, Session};
use crate::dependency::{Installer, Error as DependencyError, Explanation};
use crate::dependencies::debian::{DebianDependency, TieBreaker, default_tie_breakers};

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
    session: &'a dyn Session,
    prefix: Vec<String>,
    searchers: Option<Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>>>,
}

impl<'a> AptManager<'a> {
    pub fn new(session: &'a dyn Session, prefix: Option<Vec<String>>) -> Self {
        Self {
            session,
            prefix: prefix.unwrap_or_default(),
            searchers: None,
        }
    }

    /// Get the list of file searchers
    pub fn searchers(&'a mut self) -> &Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>> {
        if self.searchers.is_none() {
            self.searchers = Some(vec![
                crate::debian::file_search::get_apt_contents_file_searcher(self.session).unwrap(),
                Box::new(crate::debian::file_search::GENERATED_FILE_SEARCHER.clone()),
            ]);
        }
        self.searchers.as_ref().unwrap()
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

pub fn dependency_to_deb_dependency(dep: &dyn crate::dependency::Dependency, tie_breakers: &[Box<dyn TieBreaker>]) -> Result<Option<DebianDependency>, crate::dependency::Error> {

    let mut candidates = vec![]; // TODO

    Ok(pick_best_deb_dependency(candidates, tie_breakers))
}

struct AptInstaller<'a> {
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
    fn install(&self, dep: &dyn crate::dependency::Dependency, scope: crate::dependency::InstallationScope) -> Result<(), crate::dependency::Error> {
        if dep.present(self.apt.session) {
            return Ok(());
        }

        let apt_deb = if let Some(apt_deb) = dependency_to_deb_dependency(dep, &mut self.tie_breakers.as_slice())? {
            apt_deb
        } else {
            return Err(crate::dependency::Error::UnknownDependencyFamily);
        };

        match self.apt.satisfy(vec![apt_deb.relation_string().as_str()]) {
            Ok(_) => {},
            Err(e) => { return Err(crate::dependency::Error::Other(e.to_string())); }
        }
        Ok(())
    }

    fn explain(&self, dep: &dyn crate::dependency::Dependency, _scope: crate::dependency::InstallationScope) -> Result<crate::dependency::Explanation, crate::dependency::Error> {
        let apt_deb = if let Some(apt_deb) = dependency_to_deb_dependency(dep, &mut self.tie_breakers.as_slice())? {
            apt_deb
        } else {
            return Err(crate::dependency::Error::UnknownDependencyFamily);
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
