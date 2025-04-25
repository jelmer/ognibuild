use buildlog_consultant::{Match, Problem};
use log::{info, warn};
use std::fmt::{Debug, Display};

/// A fixer is a struct that can resolve a specific type of problem.
pub trait BuildFixer<O: std::error::Error>: std::fmt::Debug + std::fmt::Display {
    /// Check if this fixer can potentially resolve the given problem.
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    /// Attempt to resolve the given problem.
    fn fix(&self, problem: &dyn Problem) -> Result<bool, InterimError<O>>;
}

#[derive(Debug)]
/// Intermediate error type used during build fixing.
///
/// This enum represents different kinds of errors that can occur during
/// the build process, and which may be fixable by a BuildFixer.
pub enum InterimError<O: std::error::Error> {
    /// A problem that was detected during the build, and that we can attempt to fix.
    Recognized(Box<dyn Problem>),

    /// An error that we could not identify.
    Unidentified {
        /// The return code of the failed command.
        retcode: i32,
        /// The output lines from the command.
        lines: Vec<String>,
        /// Optional secondary information about the error.
        secondary: Option<Box<dyn Match>>,
    },

    /// Another error raised specifically by the callback function that is not fixable.
    Other(O),
}

/// Error result from repeatedly running and attemptin to fix issues.
#[derive(Debug)]
pub enum IterateBuildError<O> {
    /// The limit of fixing attempts was reached.
    FixerLimitReached(usize),

    /// A problem was detected that was recognized but could not be fixed.
    Persistent(Box<dyn Problem>),

    /// An error that we could not identify.
    Unidentified {
        /// The return code of the failed command.
        retcode: i32,
        /// The output lines from the command.
        lines: Vec<String>,
        /// Optional secondary information about the error.
        secondary: Option<Box<dyn Match>>,
    },

    /// Another error raised specifically by the callback function that is not fixable.
    Other(O),
}

impl<O: Display> Display for IterateBuildError<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterateBuildError::FixerLimitReached(limit) => {
                write!(f, "Fixer limit reached: {}", limit)
            }
            IterateBuildError::Persistent(p) => {
                write!(f, "Persistent build problem: {}", p)
            }
            IterateBuildError::Unidentified {
                retcode,
                lines,
                secondary,
            } => write!(
                f,
                "Unidentified error: retcode: {}, lines: {:?}, secondary: {:?}",
                retcode, lines, secondary
            ),
            IterateBuildError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl<O: std::error::Error> std::error::Error for IterateBuildError<O> {}

/// Call cb() until there are no more DetailedFailures we can fix.
///
/// # Arguments
/// * `fixers`: List of fixers to use to resolve issues
/// * `cb`: Callable to run the build
/// * `limit: Maximum number of fixing attempts before giving up
pub fn iterate_with_build_fixers<
    T,
    // The error type that the fixers can return.
    I: std::error::Error,
    // The error type that the callback function can return, and the eventual return type.
    E: From<I> + std::error::Error,
>(
    fixers: &[&dyn BuildFixer<I>],
    mut cb: impl FnMut() -> Result<T, InterimError<E>>,
    limit: Option<usize>,
) -> Result<T, IterateBuildError<E>> {
    let mut attempts = 0;
    let mut fixed_errors: std::collections::HashSet<Box<dyn Problem>> =
        std::collections::HashSet::new();
    loop {
        let mut to_resolve: Vec<Box<dyn Problem>> = vec![];

        match cb() {
            Ok(v) => return Ok(v),
            Err(InterimError::Recognized(e)) => to_resolve.push(e),
            Err(InterimError::Unidentified {
                retcode,
                lines,
                secondary,
            }) => {
                return Err(IterateBuildError::Unidentified {
                    retcode,
                    lines,
                    secondary,
                });
            }
            Err(InterimError::Other(e)) => return Err(IterateBuildError::Other(e)),
        }

        while let Some(f) = to_resolve.pop() {
            info!("Identified error: {:?}", f);
            if fixed_errors.contains(&f) {
                warn!("Failed to resolve error {:?}, it persisted. Giving up.", f);
                return Err(IterateBuildError::Persistent(f));
            }
            attempts += 1;
            if let Some(limit) = limit {
                if limit <= attempts {
                    return Err(IterateBuildError::FixerLimitReached(limit));
                }
            }
            match resolve_error(f.as_ref(), fixers) {
                Err(InterimError::Recognized(n)) => {
                    info!("New error {:?} while resolving {:?}", &n, &f);
                    if to_resolve.contains(&n) {
                        return Err(IterateBuildError::Persistent(n));
                    }
                    to_resolve.push(f);
                    to_resolve.push(n);
                }
                Err(InterimError::Unidentified {
                    retcode,
                    lines,
                    secondary,
                }) => {
                    return Err(IterateBuildError::Unidentified {
                        retcode,
                        lines,
                        secondary,
                    });
                }
                Err(InterimError::Other(e)) => return Err(IterateBuildError::Other(e.into())),
                Ok(resolved) if !resolved => {
                    warn!("Failed to find resolution for error {:?}. Giving up.", f);
                    return Err(IterateBuildError::Persistent(f));
                }
                Ok(_) => {
                    fixed_errors.insert(f);
                }
            }
        }
    }
}

/// Attempt to resolve a problem using available fixers.
///
/// # Arguments
/// * `problem` - The problem to resolve
/// * `fixers` - List of fixers to try
///
/// # Returns
/// * `Ok(true)` - If the problem was fixed
/// * `Ok(false)` - If no fixer could fix the problem
/// * `Err(InterimError)` - If fixing the problem failed
pub fn resolve_error<O: std::error::Error>(
    problem: &dyn Problem,
    fixers: &[&dyn BuildFixer<O>],
) -> Result<bool, InterimError<O>> {
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
        let made_changes = fixer.fix(problem)?;
        if made_changes {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Run a command repeatedly, attempting to fix any problems that occur.
///
/// This function runs a command and applies fixes if it fails,
/// potentially retrying multiple times with different fixers.
///
/// # Arguments
/// * `fixers` - List of fixers to try if the command fails
/// * `limit` - Optional maximum number of fix attempts
/// * `session` - The session to run the command in
/// * `args` - The command and its arguments
/// * `quiet` - Whether to suppress output
/// * `cwd` - Optional current working directory
/// * `user` - Optional user to run as
/// * `env` - Optional environment variables
///
/// # Returns
/// * `Ok(Vec<String>)` - The output lines if successful
/// * `Err(IterateBuildError)` - If the command fails and can't be fixed
pub fn run_fixing_problems<
    // The error type that the fixers can return.
    I: std::error::Error,
    // The error type that the callback function can return.
    E: From<I> + std::error::Error + From<std::io::Error>,
>(
    fixers: &[&dyn BuildFixer<I>],
    limit: Option<usize>,
    session: &dyn crate::session::Session,
    args: &[&str],
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
) -> Result<Vec<String>, IterateBuildError<E>> {
    iterate_with_build_fixers::<Vec<String>, I, E>(
        fixers,
        || {
            crate::analyze::run_detecting_problems(
                session,
                args.to_vec(),
                None,
                quiet,
                cwd,
                user,
                env,
                None,
            )
            .map_err(|e| match e {
                crate::analyze::AnalyzedError::Detailed { retcode: _, error } => {
                    InterimError::Recognized(error)
                }
                crate::analyze::AnalyzedError::Unidentified {
                    retcode,
                    lines,
                    secondary,
                } => InterimError::Unidentified {
                    retcode,
                    lines,
                    secondary,
                },
                crate::analyze::AnalyzedError::MissingCommandError { command } => {
                    InterimError::Recognized(Box::new(
                        buildlog_consultant::problems::common::MissingCommand(command),
                    ))
                }
                crate::analyze::AnalyzedError::IoError(e) => InterimError::Other(e.into()),
            })
        },
        limit,
    )
    .map_err(|e| match e {
        IterateBuildError::Other(e) => IterateBuildError::Other(e.into()),
        e => e,
    })
}
