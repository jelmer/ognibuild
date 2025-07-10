//! Support for vague dependencies that are not tied to a specific system.
//!
//! This module provides functionality for representing and resolving dependencies
//! that are specified in a vague manner (e.g., "zlib" without specifying whether
//! it's a binary, library, etc.). These dependencies are expanded into more
//! specific dependencies when resolved.

#[cfg(feature = "debian")]
use crate::dependencies::debian::DebianDependency;
#[cfg(feature = "debian")]
use crate::dependencies::python::PythonPackageDependency;
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
            #[cfg(feature = "debian")]
            {
                ret.push(Box::new(
                    if let Some(minimum_version) = &self.minimum_version {
                        DebianDependency::new_with_min_version(
                            &self.name,
                            &minimum_version.parse().unwrap(),
                        )
                    } else {
                        DebianDependency::new(&self.name)
                    },
                ));
                let devname = if lcname.starts_with("lib") {
                    format!("{}-dev", lcname)
                } else {
                    format!("lib{}-dev", lcname)
                };
                ret.push(if let Some(minimum_version) = &self.minimum_version {
                    Box::new(DebianDependency::new_with_min_version(
                        &devname,
                        &minimum_version.parse().unwrap(),
                    ))
                } else {
                    Box::new(DebianDependency::new(&devname))
                });
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

#[cfg(feature = "debian")]
fn known_vague_dep_to_debian(name: &str) -> Option<&str> {
    match name {
        "the Gnu Scientific Library" => Some("libgsl-dev"),
        "the required FreeType library" => Some("libfreetype-dev"),
        "the Boost C++ libraries" => Some("libboost-dev"),
        "the sndfile library" => Some("libsndfile-dev"),
        // TODO(jelmer): Support resolving virtual packages
        "PythonLibs" => Some("libpython3-dev"),
        "PythonInterp" => Some("python3"),
        "ZLIB" => Some("libz3-dev"),
        "Osmium" => Some("libosmium2-dev"),
        "glib" => Some("libglib2.0-dev"),
        "OpenGL" => Some("libgl-dev"),
        // TODO(jelmer): For Python, check minimum_version and map to python 2 or python 3
        "Python" => Some("libpython3-dev"),
        "Lua" => Some("liblua5.4-dev"),
        _ => None,
    }
}

#[cfg(feature = "debian")]
fn resolve_vague_dep_req(
    apt_mgr: &crate::debian::apt::AptManager,
    req: VagueDependency,
) -> Vec<DebianDependency> {
    let name = req.name.as_str();
    let mut options = vec![];
    if name.contains(" or ") {
        for entry in name.split(" or ") {
            options.extend(resolve_vague_dep_req(
                apt_mgr,
                VagueDependency {
                    name: entry.to_string(),
                    minimum_version: req.minimum_version.clone(),
                },
            ));
        }
    }

    if let Some(dep) = known_vague_dep_to_debian(name) {
        options.push(
            if let Some(minimum_version) = req.minimum_version.as_ref() {
                DebianDependency::new_with_min_version(dep, &minimum_version.parse().unwrap())
            } else {
                DebianDependency::new(dep)
            },
        );
    }
    for x in req.expand() {
        options.extend(crate::debian::apt::dependency_to_possible_deb_dependencies(
            apt_mgr,
            x.as_ref(),
        ));
    }

    if let Some(rest) = name.strip_prefix("GNU ") {
        options.extend(resolve_vague_dep_req(
            apt_mgr,
            VagueDependency::simple(rest),
        ));
    }

    if name.starts_with("py") || name.ends_with("py") {
        // TODO(jelmer): Try harder to determine whether this is a python package
        let dep = if let Some(min_version) = req.minimum_version.as_ref() {
            PythonPackageDependency::new_with_min_version(name, min_version)
        } else {
            PythonPackageDependency::simple(name)
        };
        options.extend(crate::debian::apt::dependency_to_possible_deb_dependencies(
            apt_mgr, &dep,
        ));
    }

    // Try even harder
    if options.is_empty() {
        use std::path::Path;

        let paths = [
            Path::new("/usr/lib")
                .join(".*")
                .join("pkgconfig")
                .join(format!("{}-.*\\.pc", regex::escape(&req.name))),
            Path::new("/usr/lib/pkgconfig").join(format!("{}-.*\\.pc", regex::escape(&req.name))),
        ];

        options.extend(
            apt_mgr
                .get_packages_for_paths(
                    paths.iter().map(|x| x.to_str().unwrap()).collect(),
                    true,
                    true,
                )
                .unwrap()
                .iter()
                .map(|x| DebianDependency::new(x)),
        )
    }

    options
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for VagueDependency {
    fn try_into_debian_dependency(
        &self,
        apt_mgr: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(resolve_vague_dep_req(apt_mgr, self.clone()))
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
