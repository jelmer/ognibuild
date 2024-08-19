use crate::buildlog::install_missing_reqs;
use crate::fix_build::BuildFixer;
use crate::requirements::BinaryRequirement;
use crate::resolver::Resolver;
use crate::session::{which, Session};
use crate::Requirement;
use std::path::{Path, PathBuf};

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    // Build: necessary to build the package
    Build,
    // core: necessary to do anything with the package
    Core,
    // test: necessary to run the tests
    Test,
    // dev: necessary for development (e.g. linters, yacc)
    Dev,
}

pub fn guaranteed_which(session: &dyn Session, resolver: &dyn Resolver, name: &str) -> PathBuf {
    match which(session, name) {
        Some(path) => PathBuf::from(path),
        None => {
            resolver.install(&[&BinaryRequirement::new(name)]);
            PathBuf::from(which(session, name).unwrap())
        }
    }
}

fn get_necessary_declared_requirements<'a>(
    resolver: &'_ dyn Resolver,
    requirements: &'_ [(Stage, &'a dyn Requirement)],
    stages: &'_ [Stage],
) -> Vec<&'a dyn Requirement> {
    let mut missing = vec![];
    for (stage, req) in requirements {
        if stages.contains(stage) {
            missing.push(*req);
        }
    }
    missing
}

/// A particular buildsystem.
pub trait BuildSystem {
    /// The name of the buildsystem.
    fn name(&self) -> &str;

    fn dist(
        &self,
        session: &dyn Session,
        resolver: &dyn Resolver,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<(), String>;

    fn install_declared_requirements(
        &self,
        stages: &[Stage],
        session: &dyn Session,
        resolver: &dyn Resolver,
        fixers: Option<&[&dyn BuildFixer]>,
    ) {
        let declared_reqs = self.get_declared_dependencies(session, fixers);
        let relevant =
            get_necessary_declared_requirements(resolver, declared_reqs.as_slice(), stages);
        install_missing_reqs(session, resolver, &relevant);
    }

    fn test(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), String>;

    fn build(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), String>;

    fn clean(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), String>;

    fn install(
        &self,
        session: &dyn Session,
        resolver: &dyn Resolver,
        install_target: &Path,
    ) -> Result<(), String>;

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer]>,
    ) -> Vec<(Stage, &dyn Requirement)>;

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer]>,
    ) -> Vec<PathBuf>;

    fn probe(path: &Path) -> Option<Box<dyn BuildSystem>>
    where
        Self: Sized,
    {
        None
    }
}
