use buildlog_consultant::{Match, Problem};
use log::{info, warn};
use std::fmt::{Debug, Display};

/// A fixer is a struct that can resolve a specific type of problem.
pub trait BuildFixer<O: std::error::Error>: std::fmt::Debug + std::fmt::Display {
    /// Check if this fixer can potentially resolve the given problem.
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    /// Attempt to resolve the given problem.
    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, InterimError<O>>;
}

#[derive(Debug)]
pub enum InterimError<O: std::error::Error> {
    /// A problem that was detected during the build, and that we can attempt to fix.
    Recognized(Box<dyn Problem>),

    /// An error that we could not identify.
    Unidentified {
        retcode: i32,
        lines: Vec<String>,
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
        retcode: i32,
        lines: Vec<String>,
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
    phase: &[&str],
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
            match resolve_error(f.as_ref(), phase, fixers) {
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

pub fn resolve_error<O: std::error::Error>(
    problem: &dyn Problem,
    phase: &[&str],
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
        let made_changes = fixer.fix(problem, phase)?;
        if made_changes {
            return Ok(true);
        }
    }
    Ok(false)
}

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
        &["build"],
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
                None,
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
