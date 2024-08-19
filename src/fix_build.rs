use crate::session::{run_with_tee, Error as SessionError, Session};
use buildlog_consultant::problems::common::MissingCommand;
use buildlog_consultant::Problem;
use log::{info, warn};
use std::fmt::{Debug, Display};
use std::process::Stdio;

pub trait BuildFixer: std::fmt::Debug + std::fmt::Display {
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error>;
}

#[derive(Debug)]
pub enum Error {
    BuildProblem(Box<dyn buildlog_consultant::Problem>),
    Other(Box<dyn std::error::Error>),
}

impl From<AnalyzedError> for Error {
    fn from(e: AnalyzedError) -> Self {
        match e {
            AnalyzedError::MissingCommandError { command } => {
                Error::BuildProblem(Box::new(MissingCommand(command)))
            }
            AnalyzedError::IoError(e) => Error::Other(Box::new(e)),
            AnalyzedError::Detailed {
                retcode: _,
                args: _,
                error,
            } => Error::BuildProblem(error.unwrap()),
            AnalyzedError::Unidentified {
                retcode: _,
                args: _,
                lines: _,
                secondary: _,
            } => Error::Other(Box::new(e)),
        }
    }
}

#[derive(Debug)]
pub enum IterateBuildError {
    FixerLimitReached(usize),
    PersistentBuildProblem(Box<dyn Problem>),
    Other(Box<dyn std::error::Error>),
}

impl From<Error> for IterateBuildError {
    fn from(e: Error) -> Self {
        match e {
            Error::BuildProblem(_) => unreachable!(),
            Error::Other(e) => IterateBuildError::Other(e),
        }
    }
}

impl From<Box<dyn Problem>> for Error {
    fn from(e: Box<dyn Problem>) -> Self {
        Error::BuildProblem(e)
    }
}

impl From<Box<dyn std::error::Error>> for Error {
    fn from(e: Box<dyn std::error::Error>) -> Self {
        Error::Other(e)
    }
}

/// Call cb() until there are no more DetailedFailures we can fix.
///
/// # Arguments
/// * `fixers`: List of fixers to use to resolve issues
/// * `cb`: Callable to run the build
/// * `limit: Maximum number of fixing attempts before giving up
pub fn iterate_with_build_fixers<T>(
    fixers: &[&dyn BuildFixer],
    phase: &[&str],
    mut cb: impl FnMut() -> Result<T, Error>,
    limit: Option<usize>,
) -> Result<T, IterateBuildError> {
    let mut attempts = 0;
    let mut fixed_errors: std::collections::HashSet<Box<dyn Problem>> =
        std::collections::HashSet::new();
    loop {
        let mut to_resolve: Vec<Box<dyn Problem>> = vec![];

        match cb() {
            Ok(v) => return Ok(v),
            Err(Error::BuildProblem(e)) => to_resolve.push(e),
            Err(e) => return Err(e.into()),
        }

        while let Some(f) = to_resolve.pop() {
            info!("Identified error: {:?}", f);
            if fixed_errors.contains(f.as_ref()) {
                warn!("Failed to resolve error {:?}, it persisted. Giving up.", f);
                return Err(IterateBuildError::PersistentBuildProblem(f));
            }
            attempts += 1;
            if let Some(limit) = limit {
                if limit <= attempts {
                    return Err(IterateBuildError::FixerLimitReached(limit));
                }
            }
            match resolve_error(f.as_ref(), phase, fixers) {
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

pub fn resolve_error(
    problem: &dyn Problem,
    phase: &[&str],
    fixers: &[&dyn BuildFixer],
) -> Result<bool, Error> {
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

/// Errors that can occur while analyzing a build.
#[derive(Debug)]
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

impl std::fmt::Display for AnalyzedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AnalyzedError::MissingCommandError { command } => {
                write!(f, "Command not found: {}", command)
            }
            AnalyzedError::IoError(e) => write!(f, "IO error: {}", e),
            AnalyzedError::Detailed {
                retcode,
                args,
                error,
            } => {
                write!(f, "Detailed error: retcode: {}, args: {:?}", retcode, args)?;
                if let Some(error) = error {
                    write!(f, ", error: {}", error)?;
                }
                Ok(())
            }
            AnalyzedError::Unidentified {
                retcode,
                args,
                lines,
                secondary,
            } => {
                write!(
                    f,
                    "Unidentified error: retcode: {}, args: {:?}, lines: {:?}",
                    retcode, args, lines
                )?;
                if let Some(secondary) = secondary {
                    write!(f, ", secondary: {:?}", secondary)?;
                }
                Ok(())
            }
        }
    }
}

impl std::error::Error for AnalyzedError {}

impl From<std::io::Error> for AnalyzedError {
    fn from(e: std::io::Error) -> Self {
        AnalyzedError::IoError(e)
    }
}

/// Run a command and analyze the output for common build problems.
///
/// # Arguments
/// * `session`: Session to run the command in
/// * `args`: Arguments to the command
/// * `check_success`: Function to determine if the command was successful
/// * `quiet`: Whether to log the command being run
/// * `cwd`: Current working directory for the command
/// * `user`: User to run the command as
/// * `env`: Environment variables to set for the command
/// * `stdin`: Stdin for the command
/// * `stdout`: Stdout for the command
/// * `stderr`: Stderr for the command
/// # Returns
/// * `Ok`: The output of the command
/// * `Err`: An error occurred while running the command
///    * `AnalyzedError::MissingCommandError`: The command was not found
///    * `AnalyzedError::IoError`: An IO error occurred
///    * `AnalyzedError::Detailed`: A detailed error occurred
///    * `AnalyzedError::Unidentified`: An unidentified error occurred
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

pub fn run_with_build_fixers(
    fixers: &[&dyn BuildFixer],
    session: &dyn Session,
    args: Vec<&str>,
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<std::collections::HashMap<String, String>>,
    stdin: Option<impl Into<Stdio> + Clone>,
    stdout: Option<impl Into<Stdio> + Clone>,
    stderr: Option<impl Into<Stdio> + Clone>,
) -> Result<Vec<String>, IterateBuildError> {
    iterate_with_build_fixers(
        fixers,
        &["build"],
        move || {
            run_detecting_problems(
                session,
                args.clone(),
                None,
                quiet,
                cwd,
                user,
                env.clone(),
                stdin.clone().map(|s| s.into()),
                stdout.clone().map(|s| s.into()),
                stderr.clone().map(|s| s.into()),
            )
            .map_err(|e| e.into())
        },
        None,
    )
}
