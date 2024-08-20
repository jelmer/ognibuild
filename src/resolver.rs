use crate::Requirement;

#[derive(Debug)]
pub enum Error {
    UnsatisfiedRequirements(Vec<Box<dyn Requirement>>),
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::UnsatisfiedRequirements(reqs) => {
                write!(f, "Unsatisfied requirements: {:?}", reqs)
            }
        }
    }
}

impl std::error::Error for Error {}

type Explain = Vec<Vec<String>>;

pub trait Resolver: std::fmt::Debug {
    fn name(&self) -> &str;

    fn install(&self, requirements: &[&dyn Requirement]) -> Result<(), Error>;

    fn resolve(&self, requirement: &dyn Requirement)
        -> Result<Option<Box<dyn Requirement>>, Error>;

    fn resolve_all(
        &self,
        requirement: &dyn Requirement,
    ) -> Result<Vec<Box<dyn Requirement>>, Error>;

    fn explain(&self, requirements: &[&dyn Requirement]) -> Result<Explain, Error>;

    fn env(&self) -> std::collections::HashMap<String, String>;
}
