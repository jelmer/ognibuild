use log::{info, warn};
use std::fmt::{Debug, Display};
use buildlog_consultant::Problem;

pub trait BuildFixer<O>: std::fmt::Debug + std::fmt::Display {
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<O>>;
}

#[derive(Debug)]
pub enum Error<O> {
    BuildProblem(Box<dyn Problem>),
    Other(O),
}

#[derive(Debug)]
pub enum IterateBuildError<O> {
    FixerLimitReached(usize),
    PersistentBuildProblem(Box<dyn Problem>),
    Other(O),
}

impl<O> From<Error<O>> for IterateBuildError<O> {
    fn from(e: Error<O>) -> Self {
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
pub fn iterate_with_build_fixers<T, O>(
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

pub fn resolve_error<O>(
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


