use log::{info, warn};
use std::fmt::{Debug, Display};
use buildlog_consultant::{Match, Problem};

pub trait BuildFixer<O: std::error::Error>: std::fmt::Debug + std::fmt::Display {
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<O>>;
}

#[derive(Debug)]
pub enum Error<O: std::error::Error> {
    BuildProblem(Box<dyn Problem>),
    Unidentified {
        retcode: i32,
        lines: Vec<String>,
        secondary: Option<Box<dyn Match>>
    },
    Other(O),
}

#[derive(Debug)]
pub enum IterateBuildError<O> {
    FixerLimitReached(usize),
    PersistentBuildProblem(Box<dyn Problem>),
    Unidentified {
        retcode: i32,
        lines: Vec<String>,
        secondary: Option<Box<dyn Match>>
    },
    Other(O),
}

impl<O: Display> Display for IterateBuildError<O> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            IterateBuildError::FixerLimitReached(limit) => write!(f, "Fixer limit reached: {}", limit),
            IterateBuildError::PersistentBuildProblem(p) => write!(f, "Persistent build problem: {}", p),
            IterateBuildError::Unidentified { retcode, lines, secondary } => write!(f, "Unidentified error: retcode: {}, lines: {:?}, secondary: {:?}", retcode, lines, secondary),
            IterateBuildError::Other(e) => write!(f, "Other error: {}", e),
        }
    }
}

impl<O: std::error::Error> std::error::Error for IterateBuildError<O> {}

impl<O: std::error::Error> From<Error<O>> for IterateBuildError<O> {
    fn from(e: Error<O>) -> Self {
        match e {
            Error::BuildProblem(_) => unreachable!(),
            Error::Other(e) => IterateBuildError::Other(e),
            Error::Unidentified { retcode, lines, secondary } => IterateBuildError::Unidentified { retcode, lines, secondary },
        }
    }
}

/// Call cb() until there are no more DetailedFailures we can fix.
///
/// # Arguments
/// * `fixers`: List of fixers to use to resolve issues
/// * `cb`: Callable to run the build
/// * `limit: Maximum number of fixing attempts before giving up
pub fn iterate_with_build_fixers<T, O: std::error::Error>(
    fixers: &[&dyn BuildFixer<O>],
    phase: &[&str],
    mut cb: impl FnMut() -> Result<T, Error<O>>,
    limit: Option<usize>,
) -> Result<T, IterateBuildError<O>> {
    let mut attempts = 0;
    let mut fixed_errors: std::collections::HashSet<Box<dyn Problem>> = std::collections::HashSet::new();
    loop {
        let mut to_resolve: Vec<Box<dyn Problem>> = vec![];

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
            match resolve_error(f.as_ref(), phase, fixers) {
                Err(Error::BuildProblem(n)) => {
                    info!("New error {:?} while resolving {:?}", &n, &f);
                    if to_resolve.contains(&n) {
                        return Err(IterateBuildError::PersistentBuildProblem(n));
                    }
                    to_resolve.push(f);
                    to_resolve.push(n);
                }
                Err(Error::Unidentified { retcode, lines, secondary }) => {
                    return Err(IterateBuildError::Unidentified { retcode, lines, secondary });
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

pub fn resolve_error<O: std::error::Error>(
    problem: &dyn Problem,
    phase: &[&str],
    fixers: &[&dyn BuildFixer<O>],
) -> Result<bool, Error<O>> {
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

pub fn run_fixing_problems<O: std::error::Error + From<crate::analyze::AnalyzedError>>(
    fixers: &[&dyn BuildFixer<O>],
    limit: Option<usize>,
    session: &dyn crate::session::Session,
    args: &[&str],
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
) -> Result<Vec<String>, IterateBuildError<O>> {
    iterate_with_build_fixers::<Vec<String>, O>(
        fixers,
        &["build"],
        || {
            crate::analyze::run_detecting_problems(session, args.to_vec(), None, quiet, cwd, user, env, None, None, None)
                .map_err(|e| match e {
                    crate::analyze::AnalyzedError::Detailed { retcode: _, error } => Error::BuildProblem(error),
                    crate::analyze::AnalyzedError::Unidentified { retcode, lines, secondary } => Error::Unidentified {
                        retcode,
                        lines,
                        secondary,
                    },
                    e => Error::Other(e.into()),
                })},
        limit,
    ).map_err(|e| match e {
        IterateBuildError::FixerLimitReached(_) => IterateBuildError::FixerLimitReached(limit.unwrap()),
        IterateBuildError::PersistentBuildProblem(p) => IterateBuildError::PersistentBuildProblem(p),
        IterateBuildError::Unidentified { retcode, lines, secondary } => IterateBuildError::Unidentified { retcode, lines, secondary },
        IterateBuildError::Other(e) => IterateBuildError::Other(e.into()),
    })
}
