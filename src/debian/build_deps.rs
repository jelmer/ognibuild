//! Debian build dependency handling.
//!
//! This module provides functionality for handling Debian build dependencies,
//! including tie-breaking between multiple potential dependencies.

use crate::dependencies::debian::DebianDependency;
use crate::dependencies::debian::TieBreaker;
use crate::session::Session;
use breezyshim::debian::apt::{Apt, LocalApt};
use std::cell::RefCell;
use std::collections::HashMap;

/// Tie-breaker for Debian build dependencies.
///
/// This tie-breaker selects the most commonly used dependency based on
/// analyzing build dependencies across all source packages in the APT cache.
pub struct BuildDependencyTieBreaker {
    /// Local APT instance for accessing package information
    apt: LocalApt,
    /// Cached counts of build dependency usage
    counts: RefCell<Option<HashMap<String, i32>>>,
}

impl BuildDependencyTieBreaker {
    /// Create a new BuildDependencyTieBreaker from a session.
    ///
    /// # Arguments
    /// * `session` - Session to use for accessing the local APT cache
    ///
    /// # Returns
    /// A new BuildDependencyTieBreaker instance
    pub fn from_session(session: &dyn Session) -> Self {
        Self {
            apt: LocalApt::new(Some(&session.location())).unwrap(),
            counts: RefCell::new(None),
        }
    }

    /// Count the occurrences of each build dependency across all source packages.
    ///
    /// This method scans all source packages in the APT cache and counts how many
    /// times each package is used as a build dependency.
    ///
    /// # Returns
    /// HashMap mapping package names to their usage count
    fn count(&self) -> HashMap<String, i32> {
        let mut counts = HashMap::new();
        for source in self.apt.iter_sources() {
            source
                .build_depends()
                .into_iter()
                .chain(source.build_depends_indep().into_iter())
                .chain(source.build_depends_arch().into_iter())
                .for_each(|r| {
                    for e in r.entries() {
                        e.relations().for_each(|r| {
                            let count = counts.entry(r.name().clone()).or_insert(0);
                            *count += 1;
                        });
                    }
                });
        }
        counts
    }
}

/// Implementation of TieBreaker for BuildDependencyTieBreaker.
impl TieBreaker for BuildDependencyTieBreaker {
    /// Break a tie between multiple Debian dependencies by selecting the most commonly used one.
    ///
    /// # Arguments
    /// * `reqs` - Slice of Debian dependency candidates to choose from
    ///
    /// # Returns
    /// The most commonly used dependency, or None if no candidates are available
    fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency> {
        if self.counts.borrow().is_none() {
            let counts = self.count();
            self.counts.replace(Some(counts));
        }

        let c = self.counts.borrow();
        let count = c.clone().unwrap();
        let mut by_count = HashMap::new();
        for req in reqs {
            let name = req.package_names().into_iter().next().unwrap();
            by_count.insert(req, count[&name]);
        }
        if by_count.is_empty() {
            return None;
        }
        let top = by_count.iter().max_by_key(|k| k.1).unwrap();
        log::info!(
            "Breaking tie between [{:?}] to {:?} based on build-depends count",
            reqs.iter().map(|r| r.relation_string()).collect::<Vec<_>>(),
            top.0.relation_string(),
        );
        Some(*top.0)
    }
}
