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

    let apt_mgr = apt::AptManager::new(session, None);
    apt_mgr.satisfy(build_dep_entries(&control))
}

/// Satisfy build dependencies parsed from a debian/control file on disk.
///
/// Unlike [`satisfy_build_deps`], this reads the control file directly from the
/// filesystem rather than from a VCS tree, which is useful when indexing an
/// unpacked source package that has no version control metadata. Dependencies
/// are satisfied in `session` via apt.
///
/// # Arguments
/// * `session` - Session to run apt in
/// * `control_path` - Path to the debian/control file (on the host filesystem)
pub fn satisfy_build_deps_from_control(
    session: &dyn Session,
    control_path: &std::path::Path,
) -> Result<(), Box<dyn std::error::Error>> {
    let text = std::fs::read_to_string(control_path)?;
    let control: debian_control::Control = text.parse()?;
    let apt_mgr = apt::AptManager::new(session, None);
    apt_mgr
        .satisfy(build_dep_entries(&control))
        .map_err(|e| format!("Failed to satisfy build dependencies: {:?}", e).into())
}

/// Collect the build dependencies and conflicts from a parsed control file as
/// apt satisfy entries.
fn build_dep_entries(control: &debian_control::Control) -> Vec<apt::SatisfyEntry> {
    let Some(source) = control.source() else {
        return vec![];
    };

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

    deps
}

#[cfg(test)]
mod tests {
    use super::*;

    fn classify(entries: &[apt::SatisfyEntry]) -> (Vec<&str>, Vec<&str>) {
        let mut required = vec![];
        let mut conflicts = vec![];
        for entry in entries {
            match entry {
                apt::SatisfyEntry::Required(s) => required.push(s.as_str()),
                apt::SatisfyEntry::Conflict(s) => conflicts.push(s.as_str()),
            }
        }
        (required, conflicts)
    }

    #[test]
    fn test_build_dep_entries() {
        let control: debian_control::Control = "Source: dulwich\n\
            Build-Depends: debhelper-compat (= 13), dh-python\n\
            Build-Depends-Indep: python3-sphinx\n\
            Build-Conflicts: python3-broken\n\n\
            Package: python3-dulwich\n\
            Architecture: any\n"
            .parse()
            .unwrap();
        let entries = build_dep_entries(&control);
        let (required, conflicts) = classify(&entries);
        // Each field is emitted as a single comma-separated relation string,
        // which is what "apt satisfy" expects.
        assert_eq!(
            required,
            vec!["debhelper-compat (= 13), dh-python", "python3-sphinx"]
        );
        assert_eq!(conflicts, vec!["python3-broken"]);
    }

    #[test]
    fn test_build_dep_entries_no_source() {
        let control: debian_control::Control =
            "Package: python3-dulwich\nArchitecture: any\n".parse().unwrap();
        assert!(build_dep_entries(&control).is_empty());
    }
}
