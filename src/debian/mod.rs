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
/// The packages are downloaded with network access enabled and then installed
/// under the session's current network policy, so the install step can run in
/// an otherwise network-isolated session (e.g. while indexing offline).
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
    let deps = build_dep_entries(&control);

    // apt needs the network to fetch the packages; download them with network
    // access enabled, then install from the local cache under whatever network
    // policy the session is configured with.
    crate::session::with_network(session, || {
        apt_mgr.satisfy_phase(deps.clone(), apt::SatisfyPhase::Download)
    })
    .map_err(|e| format!("Failed to download build dependencies: {:?}", e))?;
    apt_mgr
        .satisfy_phase(deps, apt::SatisfyPhase::Install)
        .map_err(|e| format!("Failed to install build dependencies: {:?}", e))?;
    Ok(())
}

/// Clean apt-satisfy strings for each entry of a Build-Depends-style relation
/// field, one per comma-separated entry (preserving `|` alternations).
///
/// The lossless `Relations`/`Entry` Display reproduces the raw field text,
/// including comments and line continuations, which `apt satisfy` cannot parse
/// (e.g. rustc's heavily commented Build-Depends). Convert each entry to its
/// lossy relations, whose Display reconstructs the relation from its parsed
/// components (name, version, architecture and profile restrictions) without
/// comments.
fn relation_entries(relations: &debian_control::lossless::relations::Relations) -> Vec<String> {
    relations
        .entries()
        .map(|entry| {
            let alternatives: Vec<debian_control::lossy::Relation> = entry.into();
            alternatives
                .iter()
                .map(|relation| relation.to_string())
                .collect::<Vec<_>>()
                .join(" | ")
        })
        .collect()
}

/// Collect the build dependencies and conflicts from a parsed control file as
/// apt satisfy entries.
fn build_dep_entries(control: &debian_control::Control) -> Vec<apt::SatisfyEntry> {
    let Some(source) = control.source() else {
        return vec![];
    };

    let mut deps = vec![];

    for relations in source
        .build_depends()
        .iter()
        .chain(source.build_depends_indep().iter())
        .chain(source.build_depends_arch().iter())
    {
        for entry in relation_entries(relations) {
            deps.push(apt::SatisfyEntry::Required(entry));
        }
    }

    for relations in source
        .build_conflicts()
        .iter()
        .chain(source.build_conflicts_indep().iter())
        .chain(source.build_conflicts_arch().iter())
    {
        for entry in relation_entries(relations) {
            deps.push(apt::SatisfyEntry::Conflict(entry));
        }
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
        // One apt satisfy entry per comma-separated relation.
        assert_eq!(
            required,
            vec!["debhelper-compat (= 13)", "dh-python", "python3-sphinx"]
        );
        assert_eq!(conflicts, vec!["python3-broken"]);
    }

    #[test]
    fn test_build_dep_entries_no_source() {
        let control: debian_control::Control = "Package: python3-dulwich\nArchitecture: any\n"
            .parse()
            .unwrap();
        assert!(build_dep_entries(&control).is_empty());
    }

    #[test]
    fn test_build_dep_entries_strips_comments_and_continuations() {
        // A Build-Depends field with embedded comments, line continuations,
        // an alternation and a build profile, as in rustc's debian/control.
        // The raw field text would not parse as an apt dependency; the entries
        // must come out clean.
        let control_text = concat!(
            "Source: rustc\n",
            "Build-Depends: debhelper-compat (= 13),\n",
            "# needed by some vendor crates\n",
            " pkgconf:native,\n",
            " libcurl4-openssl-dev | libcurl4-gnutls-dev,\n",
            " git <!nocheck>\n",
            "\n",
            "Package: rustc\n",
            "Architecture: any\n",
        );
        let control: debian_control::Control = control_text.parse().unwrap();
        let entries = build_dep_entries(&control);
        let (required, _) = classify(&entries);
        assert_eq!(
            required,
            vec![
                "debhelper-compat (= 13)",
                "pkgconf:native",
                "libcurl4-openssl-dev | libcurl4-gnutls-dev",
                "git <!nocheck>",
            ]
        );
    }
}
