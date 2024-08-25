use std::collections::HashMap;
use breezyshim::debian::apt::{Apt,LocalApt};
use crate::session::Session;
use crate::dependencies::debian::DebianDependency;
use crate::dependencies::debian::TieBreaker;

pub struct BuildDependencyTieBreaker {
    apt: LocalApt,
    counts: Option<HashMap<String, i32>>,
}

impl BuildDependencyTieBreaker {
    pub fn from_session(session: &dyn Session) -> Self {
        Self {
            apt: LocalApt::new(Some(&session.location())).unwrap(),
            counts: None,
        }
    }

    fn count(&self) -> HashMap<String, i32> {
        let mut counts = HashMap::new();
        for source in self.apt.iter_sources() {
            source.build_depends().into_iter().chain(source.build_depends_indep().into_iter()).chain(source.build_depends_arch().into_iter()).for_each(|r| {
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

impl TieBreaker for BuildDependencyTieBreaker {
    fn break_tie<'a>(&mut self, reqs: Vec<&'a DebianDependency>) -> Option<&'a DebianDependency>{
        let counts = self.count();
        self.counts = Some(counts);
        let mut by_count = HashMap::new();
        for req in &reqs {
            let name = req.package_names().into_iter().next().unwrap();
            by_count.insert(req, self.counts.as_ref().unwrap()[&name]);
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
