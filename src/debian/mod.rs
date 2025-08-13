//! Debian packaging support for ognibuild.
//!
//! This module provides functionality for working with Debian packages,
//! including managing build dependencies, interacting with APT,
//! fixing build issues, and working with Debian package sources.

/// APT package management functionality.
pub mod apt;
/// Debian package build functionality.
pub mod build;
/// Build dependency resolution for Debian packages.
pub mod build_deps;
/// Context management for Debian operations.
pub mod context;
/// Dependency server integration.
#[cfg(feature = "dep-server")]
pub mod dep_server;
/// File search utilities for Debian packages.
pub mod file_search;
/// Debian-specific build fixing functionality.
pub mod fix_build;
/// Build fixers for Debian packages.
pub mod fixers;
/// Ultimate Debian Database integration.
#[cfg(feature = "udd")]
pub mod udd;
/// Upstream dependency handling for Debian packages.
pub mod upstream_deps;
use breezyshim::tree::{Path, Tree};

use crate::session::Session;

/// Satisfy build dependencies for a Debian package.
///
/// This function parses the debian/control file and installs all required
/// build dependencies while ensuring conflicts are resolved.
///
/// # Arguments
/// * `session` - Session to run commands in
/// * `tree` - Tree representing the package source
/// * `debian_path` - Path to the debian directory
///
/// # Returns
/// Ok on success, Error if dependencies cannot be satisfied
pub fn satisfy_build_deps(
    session: &dyn Session,
    tree: &dyn Tree,
    debian_path: &Path,
) -> Result<(), apt::Error> {
    let path = debian_path.join("control");

    let f = tree.get_file_text(&path).unwrap();

    let control: debian_control::Control = String::from_utf8(f).unwrap().parse().unwrap();

    let source = control.source().unwrap();

    let mut deps = vec![];

    for dep in source
        .build_depends()
        .iter()
        .chain(source.build_depends_indep().iter())
        .chain(source.build_depends_arch().iter())
    {
        deps.push(apt::SatisfyEntry::Required(dep.to_string()));
    }

    for dep in source
        .build_conflicts()
        .iter()
        .chain(source.build_conflicts_indep().iter())
        .chain(source.build_conflicts_arch().iter())
    {
        deps.push(apt::SatisfyEntry::Conflict(dep.to_string()));
    }

    let apt_mgr = apt::AptManager::new(session, None);
    apt_mgr.satisfy(deps)
}
