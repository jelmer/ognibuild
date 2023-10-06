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
