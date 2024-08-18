use crate::resolver::Resolver;
use crate::session::Session;
use crate::Requirement;

pub fn install_missing_reqs(
    session: &dyn Session,
    resolver: &dyn Resolver,
    reqs: &[&dyn Requirement],
) {
    if reqs.is_empty() {
        return;
    }
    let mut missing = vec![];
    for req in reqs {
        if !req.met(session) {
            missing.push(*req)
        }
    }
    if !missing.is_empty() {
        resolver.install(missing.as_slice());
    }
}

pub enum Explanation<'a> {
    Install(Vec<Vec<String>>),
    Uninstallable(Vec<&'a dyn Requirement>),
}

pub fn explain_missing_reqs<'a>(
    session: &dyn Session,
    resolver: &dyn Resolver,
    reqs: &[&'a dyn Requirement],
) -> Explanation<'a> {
    if reqs.is_empty() {
        return Explanation::Install(vec![]);
    }
    let mut missing = vec![];
    for req in reqs.into_iter() {
        if !req.met(session) {
            missing.push(*req)
        }
    }
    if !missing.is_empty() {
        let commands = resolver.explain(missing.as_slice());
        if commands.is_empty() {
            Explanation::Uninstallable(missing)
        } else {
            Explanation::Install(commands)
        }
    } else {
        Explanation::Install(vec![])
    }
}
