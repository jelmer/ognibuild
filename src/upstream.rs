//! This module provides a trait for dependencies that can find their upstream metadata.
use crate::dependency::Dependency;
pub use upstream_ontologist::UpstreamMetadata;

/// A trait for dependencies that can find their upstream metadata.
pub trait FindUpstream: Dependency {

    /// Find the upstream metadata for this dependency.
    fn find_upstream(&self) -> Option<UpstreamMetadata>;
}

pub fn find_upstream(dependency: &dyn Dependency) -> Option<UpstreamMetadata> {
    #[cfg(feature = "debian")]
    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::debian::DebianDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::python::PythonPackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::RubyGemDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::node::NodePackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::CargoCrateDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::go::GoPackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::perl::PerlModuleDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::haskell::HaskellPackageDependency>()
    {
        return dep.find_upstream();
    }

    None
}
