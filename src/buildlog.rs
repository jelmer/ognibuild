use buildlog_consultant::Problem;
use crate::dependency::Dependency;

pub trait ToDependency: Problem {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>>;
}
