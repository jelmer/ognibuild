use crate::session::{run_with_tee, Error as SessionError, Session};
use buildlog_consultant::problems::common::MissingCommand;
use log::{info, warn};
use std::fmt::{Debug, Display};

pub trait BuildFixer<O, P>: std::fmt::Debug + std::fmt::Display {
    fn can_fix(&self, problem: &P) -> bool;

    fn fix(&self, problem: &P, phase: &[&str]) -> Result<bool, Error<O, P>>;
}

#[derive(Debug)]
pub enum Error<O, P> {
    BuildProblem(P),
    Other(O),
}

#[derive(Debug)]
pub enum IterateBuildError<O, P> {
    FixerLimitReached(usize),
    PersistentBuildProblem(P),
    Other(O),
}

impl<O, P> From<Error<O, P>> for IterateBuildError<O, P> {
    fn from(e: Error<O, P>) -> Self {
        match e {
            Error::BuildProblem(_) => unreachable!(),
            Error::Other(e) => IterateBuildError::Other(e),
        }
    }
}

/// Call cb() until there are no more DetailedFailures we can fix.
///
/// # Arguments
/// * `fixers`: List of fixers to use to resolve issues
/// * `cb`: Callable to run the build
/// * `limit: Maximum number of fixing attempts before giving up
pub fn iterate_with_build_fixers<T, O, P: Debug + Display + std::hash::Hash + PartialEq + Eq>(
    fixers: &[&dyn BuildFixer<O, P>],
    phase: &[&str],
    mut cb: impl FnMut() -> Result<T, Error<O, P>>,
    limit: Option<usize>,
) -> Result<T, IterateBuildError<O, P>> {
    let mut attempts = 0;
    let mut fixed_errors: std::collections::HashSet<P> = std::collections::HashSet::new();
    loop {
        let mut to_resolve: Vec<P> = vec![];

        match cb() {
            Ok(v) => return Ok(v),
            Err(Error::BuildProblem(e)) => to_resolve.push(e),
            Err(e) => return Err(e.into()),
        }

        while let Some(f) = to_resolve.pop() {
            info!("Identified error: {:?}", f);
            if fixed_errors.contains(&f) {
                warn!("Failed to resolve error {:?}, it persisted. Giving up.", f);
                return Err(IterateBuildError::PersistentBuildProblem(f));
            }
            attempts += 1;
            if let Some(limit) = limit {
                if limit <= attempts {
                    return Err(IterateBuildError::FixerLimitReached(limit));
                }
            }
            match resolve_error(&f, phase, fixers) {
                Err(Error::BuildProblem(n)) => {
                    info!("New error {:?} while resolving {:?}", &n, &f);
                    if to_resolve.contains(&n) {
                        return Err(IterateBuildError::PersistentBuildProblem(n));
                    }
                    to_resolve.push(f);
                    to_resolve.push(n);
                }
                Err(Error::Other(e)) => return Err(IterateBuildError::Other(e)),
                Ok(resolved) if !resolved => {
                    warn!("Failed to find resolution for error {:?}. Giving up.", f);
                    return Err(IterateBuildError::PersistentBuildProblem(f));
                }
                Ok(_) => {
                    fixed_errors.insert(f);
                }
            }
        }
    }
}

pub fn resolve_error<O, P: Debug>(
    problem: &P,
    phase: &[&str],
    fixers: &[&dyn BuildFixer<O, P>],
) -> Result<bool, Error<O, P>> {
    let relevant_fixers = fixers
        .iter()
        .filter(|fixer| fixer.can_fix(problem))
        .collect::<Vec<_>>();
    if relevant_fixers.is_empty() {
        warn!("No fixer found for {:?}", problem);
        return Ok(false);
    }
    for fixer in relevant_fixers {
        info!("Attempting to use fixer {} to address {:?}", fixer, problem);
        let made_changes = fixer.fix(problem, phase)?;
        if made_changes {
            return Ok(true);
        }
    }
    Ok(false)
}

fn default_check_success(retcode: i32, _lines: Vec<&str>) -> bool {
    retcode == 0
}

pub enum AnalyzedError {
    MissingCommandError {
        command: String,
    },
    IoError(std::io::Error),
    Detailed {
        retcode: i32,
        args: Vec<String>,
        error: Option<Box<dyn buildlog_consultant::Problem>>,
    },
    Unidentified {
        retcode: i32,
        args: Vec<String>,
        lines: Vec<String>,
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
}

impl From<std::io::Error> for AnalyzedError {
    fn from(e: std::io::Error) -> Self {
        AnalyzedError::IoError(e)
    }
}

pub fn run_detecting_problems(
    session: &dyn Session,
    args: Vec<&str>,
    check_success: Option<&dyn Fn(i32, Vec<&str>) -> bool>,
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
    stdout: Option<std::process::Stdio>,
    stderr: Option<std::process::Stdio>,
) -> Result<Vec<String>, AnalyzedError> {
    if !quiet {
        log::info!("Running {:?}", args);
    }
    let check_success = check_success.unwrap_or(&default_check_success);

    let (retcode, contents) =
        match run_with_tee(session, args.clone(), cwd, user, env, stdin, stdout, stderr) {
            Ok((retcode, contents)) => (retcode, contents),
            Err(SessionError::SetupFailure(..)) => unreachable!(),
            Err(SessionError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let command = args[0].to_string();
                return Err(AnalyzedError::Detailed {
                    retcode: 127,
                    args: args.into_iter().map(|s| s.to_string()).collect(),
                    error: Some(
                        Box::new(MissingCommand(command)) as Box<dyn buildlog_consultant::Problem>
                    ),
                });
            }
            Err(SessionError::IoError(e)) => {
                return Err(AnalyzedError::IoError(e));
            }
            Err(SessionError::CalledProcessError(retcode)) => (retcode, vec![]),
        };
    if check_success(retcode, contents.iter().map(|s| s.as_str()).collect()) {
        return Ok(contents);
    }
    let body = contents.join("");
    let lines = body.split('\n').collect::<Vec<_>>();
    let (r#match, error) =
        buildlog_consultant::common::find_build_failure_description(lines.clone());
    if let Some(error) = error {
        Err(AnalyzedError::Detailed {
            retcode,
            args: args.into_iter().map(|s| s.to_string()).collect(),
            error: Some(error),
        })
    } else {
        if let Some(r#match) = r#match.as_ref() {
            log::warn!("Build failed with unidentified error:");
            log::warn!("{}", r#match.line().trim_end_matches('\n'));
        } else {
            log::warn!("Build failed and unable to find cause. Giving up.");
        }
        Err(AnalyzedError::Unidentified {
            retcode,
            args: args.into_iter().map(|s| s.to_string()).collect(),
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            secondary: r#match,
        })
    }
}
