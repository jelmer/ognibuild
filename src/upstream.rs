use crate::dependency::Dependency;

pub struct Upstream {}

pub trait FindUpstream: Dependency {
    fn find_upstream(&self) -> Option<Upstream>;
}

pub fn find_upstream(dependency: &dyn Dependency) -> Option<Upstream> {
    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::debian::DebianDependency>()
    {
        return dep.find_upstream();
    }

    None
}
