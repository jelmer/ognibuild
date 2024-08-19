use crate::Requirement;

pub trait Resolver: std::fmt::Debug {
    fn name(&self) -> &str;

    fn install(&self, requirements: &[&dyn Requirement]);

    fn resolve(&self, requirement: &dyn Requirement) -> Option<Box<dyn Requirement>>;

    fn resolve_all(&self, requirement: &dyn Requirement) -> Vec<Box<dyn Requirement>>;

    fn explain(&self, requirements: &[&dyn Requirement]) -> Vec<Vec<String>>;

    fn env(&self) -> std::collections::HashMap<String, String>;
}
