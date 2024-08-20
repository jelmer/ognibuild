use crate::fix_build::{BuildFixer, Error};
use crate::resolver::{Error as ResolverError, Resolver};
use crate::session::Session;
use crate::Requirement;
use buildlog_consultant::Problem;

pub fn problem_to_upstream_requirement(problem: &dyn Problem) -> Option<Box<dyn Requirement>> {
    // TODO
    None
}

pub fn install_missing_reqs(
    session: &dyn Session,
    resolver: &dyn Resolver,
    reqs: &[&dyn Requirement],
) -> Result<(), ResolverError> {
    if reqs.is_empty() {
        return Ok(());
    }
    let mut missing = vec![];
    for req in reqs {
        if !req.met(session) {
            missing.push(*req)
        }
    }
    if !missing.is_empty() {
        resolver.install(missing.as_slice())?;
    }

    Ok(())
}

pub enum Explanation<'a> {
    Install(Vec<Vec<String>>),
    Uninstallable(Vec<&'a dyn Requirement>),
}

pub fn explain_missing_reqs<'a>(
    session: &dyn Session,
    resolver: &dyn Resolver,
    reqs: &[&'a dyn Requirement],
) -> Result<Explanation<'a>, ResolverError> {
    if reqs.is_empty() {
        return Ok(Explanation::Install(vec![]));
    }
    let mut missing = vec![];
    for req in reqs.iter() {
        if !req.met(session) {
            missing.push(*req)
        }
    }
    if !missing.is_empty() {
        let commands = resolver.explain(missing.as_slice())?;
        if commands.is_empty() {
            Ok(Explanation::Uninstallable(missing))
        } else {
            Ok(Explanation::Install(commands))
        }
    } else {
        Ok(Explanation::Install(vec![]))
    }
}

#[derive(Debug)]
pub struct InstallFixer {
    resolver: Box<dyn Resolver>,
}

impl std::fmt::Display for InstallFixer {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "InstallFixer for {:?}", self.resolver)
    }
}

impl BuildFixer for InstallFixer {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        let req = problem_to_upstream_requirement(problem);
        req.is_some()
    }

    fn fix(&self, problem: &dyn Problem, _phase: &[&str]) -> Result<bool, Error> {
        let req = problem_to_upstream_requirement(problem);
        if req.is_none() {
            return Ok(false);
        }

        let reqs = [req.unwrap()];

        match self.resolver.install(
            reqs.iter()
                .map(|x| x.as_ref())
                .collect::<Vec<&dyn Requirement>>()
                .as_slice(),
        ) {
            Ok(_) => Ok(true),
            Err(ResolverError::UnsatisfiedRequirements(_)) => Ok(false),
        }
    }
}
