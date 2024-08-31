use crate::dependencies::debian::DebianDependency;
use crate::dependency::Dependency;

pub trait FromDebianDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>>;
}
