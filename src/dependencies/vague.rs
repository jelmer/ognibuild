//! Support for vague dependencies that are not tied to a specific system.
//!
//! This module provides functionality for representing and resolving dependencies
//! that are specified in a vague manner (e.g., "zlib" without specifying whether
//! it's a binary, library, etc.). These dependencies are expanded into more
//! specific dependencies when resolved.

use crate::dependencies::BinaryDependency;
use crate::dependencies::Dependency;
use crate::dependencies::PkgConfigDependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency that is not tied to a specific system or package manager.
///
/// This represents a dependency that could be satisfied in multiple ways.
/// When resolved, it expands into multiple specific dependency types.
pub struct VagueDependency {
    /// The name of the dependency.
    pub name: String,
    /// The minimum version required, if any.
    pub minimum_version: Option<String>,
}

impl VagueDependency {
    /// Create a new VagueDependency with the given name and optional minimum version.
    ///
    /// # Arguments
    /// * `name` - The name of the dependency
    /// * `minimum_version` - Optional minimum version requirement
    pub fn new(name: &str, minimum_version: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            minimum_version: minimum_version.map(|s| s.trim().to_string()),
        }
    }

    /// Create a simple VagueDependency with just a name and no version requirement.
    ///
    /// # Arguments
    /// * `name` - The name of the dependency
    pub fn simple(name: &str) -> Self {
        Self {
            name: name.to_string(),
            minimum_version: None,
        }
    }

    /// Expand this vague dependency into more specific dependency types.
    ///
    /// This converts the vague dependency into specific dependency types such as
    /// binary dependencies, pkg-config dependencies, and Debian dependencies.
    ///
    /// # Returns
    /// A vector of specific dependency implementations
    pub fn expand(&self) -> Vec<Box<dyn Dependency>> {
        let mut ret: Vec<Box<dyn Dependency>> = vec![];
        let lcname = self.name.to_lowercase();
        if !self.name.contains(' ') {
            ret.push(Box::new(BinaryDependency::new(&self.name)) as Box<dyn Dependency>);
            ret.push(Box::new(BinaryDependency::new(&self.name)) as Box<dyn Dependency>);
            ret.push(Box::new(PkgConfigDependency::new(
                &self.name.clone(),
                self.minimum_version.clone().as_deref(),
            )) as Box<dyn Dependency>);
            if lcname != self.name {
                ret.push(Box::new(BinaryDependency::new(&lcname)) as Box<dyn Dependency>);
                ret.push(Box::new(BinaryDependency::new(&lcname)) as Box<dyn Dependency>);
                ret.push(Box::new(PkgConfigDependency::new(
                    &lcname,
                    self.minimum_version.clone().as_deref(),
                )) as Box<dyn Dependency>);
            }
        }
        ret
    }
}

impl Dependency for VagueDependency {
    fn family(&self) -> &'static str {
        "vague"
    }

    fn present(&self, session: &dyn Session) -> bool {
        self.expand().iter().any(|d| d.present(session))
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        self.expand().iter().any(|d| d.project_present(session))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[test]
    fn test_vague_dependency_new() {
        let dep = VagueDependency::new("zlib", Some("1.2.11"));
        assert_eq!(dep.name, "zlib");
        assert_eq!(dep.minimum_version, Some("1.2.11".to_string()));
    }

    #[test]
    fn test_vague_dependency_new_trims_version() {
        let dep = VagueDependency::new("zlib", Some(" 1.2.11 "));
        assert_eq!(dep.minimum_version, Some("1.2.11".to_string()));
    }

    #[test]
    fn test_vague_dependency_simple() {
        let dep = VagueDependency::simple("zlib");
        assert_eq!(dep.name, "zlib");
        assert_eq!(dep.minimum_version, None);
    }

    #[test]
    fn test_vague_dependency_family() {
        let dep = VagueDependency::simple("zlib");
        assert_eq!(dep.family(), "vague");
    }

    #[test]
    fn test_vague_dependency_as_any() {
        let dep = VagueDependency::simple("zlib");
        let any_dep: &dyn Any = dep.as_any();
        assert!(any_dep.downcast_ref::<VagueDependency>().is_some());
    }

    #[test]
    fn test_vague_dependency_expand() {
        let dep = VagueDependency::simple("zlib");
        let expanded = dep.expand();

        // Should generate binary dependencies
        assert!(expanded.iter().any(|d| d.family() == "binary"
            && d.as_any()
                .downcast_ref::<BinaryDependency>()
                .map(|bd| bd.binary_name == "zlib")
                .unwrap_or(false)));

        // Should generate pkg-config dependencies
        assert!(expanded.iter().any(|d| d.family() == "pkg-config"
            && d.as_any()
                .downcast_ref::<PkgConfigDependency>()
                .map(|pd| pd.module == "zlib")
                .unwrap_or(false)));

        // Should also include lowercase versions
        assert!(expanded.iter().any(|d| d.family() == "binary"
            && d.as_any()
                .downcast_ref::<BinaryDependency>()
                .map(|bd| bd.binary_name == "zlib")
                .unwrap_or(false)));
    }

    #[test]
    fn test_vague_dependency_expand_with_spaces() {
        let dep = VagueDependency::simple("zlib library");
        let expanded = dep.expand();

        // Should not expand dependencies with spaces
        assert!(expanded.is_empty());
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingVagueDependency
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(VagueDependency::new(
            &self.name,
            self.minimum_version.as_deref(),
        )))
    }
}
