use crate::buildlog::install_missing_reqs;
use crate::fix_build::{BuildFixer, Error};
use crate::requirements::BinaryRequirement;
use crate::resolver::{Error as ResolverError, Resolver};
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

impl Stage {
    pub fn all() -> &'static [Stage] {
        &[Stage::Build, Stage::Core, Stage::Test, Stage::Dev]
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Stage::Build => write!(f, "build"),
            Stage::Core => write!(f, "core"),
            Stage::Test => write!(f, "test"),
            Stage::Dev => write!(f, "dev"),
        }
    }
}

pub fn guaranteed_which(session: &dyn Session, resolver: &dyn Resolver, name: &str) -> PathBuf {
    match which(session, name) {
        Some(path) => PathBuf::from(path),
        None => {
            resolver.install(&[&BinaryRequirement::new(name)]).unwrap();
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

pub struct InstallTarget {
    pub user: bool,
    pub prefix: Option<PathBuf>,
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
    ) -> Result<(), Error>;

    fn install_declared_requirements(
        &self,
        stages: &[Stage],
        session: &dyn Session,
        resolver: &dyn Resolver,
        fixers: Option<&[&dyn BuildFixer]>,
    ) -> Result<(), ResolverError> {
        let declared_reqs = self.get_declared_dependencies(session, fixers);
        let relevant =
            get_necessary_declared_requirements(resolver, declared_reqs.as_slice(), stages);
        install_missing_reqs(session, resolver, &relevant)?;
        Ok(())
    }

    fn test(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), Error>;

    fn build(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), Error>;

    fn clean(&self, session: &dyn Session, resolver: &dyn Resolver) -> Result<(), Error>;

    fn install(
        &self,
        session: &dyn Session,
        resolver: &dyn Resolver,
        install_target: &InstallTarget,
    ) -> Result<(), Error>;

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

pub fn detect_buildsystems(path: &Path) -> Vec<Box<dyn BuildSystem>> {
    vec![]
}
