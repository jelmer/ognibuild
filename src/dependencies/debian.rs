use crate::dependency::Dependency;
use crate::session::Session;
use debian_control::lossless::relations::{Entry, Relation, Relations};
use debian_control::relations::VersionConstraint;
use debversion::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::hash::Hash;

#[derive(Debug)]
pub struct DebianDependency(Relations);

impl Clone for DebianDependency {
    fn clone(&self) -> Self {
        let rels = self.0.to_string().parse().unwrap();
        DebianDependency(rels)
    }
}

impl Serialize for DebianDependency {
    fn serialize<S>(&self, serializer: S) -> Result<S::Ok, S::Error>
    where
        S: Serializer,
    {
        self.0.to_string().serialize(serializer)
    }
}

impl<'a> Deserialize<'a> for DebianDependency {
    fn deserialize<D>(deserializer: D) -> Result<Self, D::Error>
    where
        D: Deserializer<'a>,
    {
        let s = String::deserialize(deserializer)?;
        Ok(DebianDependency(s.parse().unwrap()))
    }
}

impl PartialEq for DebianDependency {
    fn eq(&self, other: &Self) -> bool {
        self.0.to_string() == other.0.to_string()
    }
}

impl Eq for DebianDependency {}

impl Hash for DebianDependency {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.0.to_string().hash(state);
    }
}

impl DebianDependency {
    /// Create a new dependency from a package name.
    pub fn new(name: &str) -> DebianDependency {
        DebianDependency(
            name.parse()
                .unwrap_or_else(|_| panic!("Failed to parse dependency: {}", name)),
        )
    }

    pub fn iter(&self) -> impl Iterator<Item = Entry> + '_ {
        self.0.entries()
    }

    pub fn relation_string(&self) -> String {
        self.0.to_string()
    }

    pub fn simple(name: &str) -> DebianDependency {
        Self::new(name)
    }

    pub fn new_with_min_version(name: &str, min_version: &Version) -> DebianDependency {
        DebianDependency(
            format!("{} (>= {})", name, min_version)
                .parse()
                .unwrap_or_else(|_| {
                    panic!("Failed to parse dependency: {} (>= {})", name, min_version)
                }),
        )
    }

    pub fn touches_package(&self, package: &str) -> bool {
        for entry in self.0.entries() {
            for relation in entry.relations() {
                if relation.name() == package {
                    return true;
                }
            }
        }
        false
    }

    pub fn package_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        for entry in self.0.entries() {
            for relation in entry.relations() {
                names.insert(relation.name());
            }
        }
        names
    }

    pub fn satisfied_by(
        &self,
        versions: &std::collections::HashMap<String, debversion::Version>,
    ) -> bool {
        let relation_satisfied = |relation: Relation| -> bool {
            let name = relation.name();
            let version = if let Some(version) = versions.get(&name) {
                version
            } else {
                return false;
            };
            match relation.version() {
                Some((VersionConstraint::Equal, v)) => version.cmp(&v) == std::cmp::Ordering::Equal,
                Some((VersionConstraint::GreaterThanEqual, v)) => version >= &v,
                Some((VersionConstraint::GreaterThan, v)) => version > &v,
                Some((VersionConstraint::LessThanEqual, v)) => version <= &v,
                Some((VersionConstraint::LessThan, v)) => version < &v,
                None => true,
            }
        };

        self.0
            .entries()
            .all(|entry| entry.relations().any(relation_satisfied))
    }
}

/// Get the version of a package installed on the system.
///
/// Returns `None` if the package is not installed.
fn get_package_version(session: &dyn Session, package: &str) -> Option<debversion::Version> {
    let argv = vec!["dpkg-query", "-W", "-f='${Version}\n'", package];
    let output = String::from_utf8(session.command(argv).check_output().unwrap()).unwrap();

    if output.trim().is_empty() {
        None
    } else {
        Some(output.trim().parse().unwrap())
    }
}

impl Dependency for DebianDependency {
    fn family(&self) -> &'static str {
        "debian"
    }

    fn present(&self, session: &dyn Session) -> bool {
        use std::collections::HashMap;
        let mut versions = HashMap::new();
        for name in self.package_names() {
            if let Some(version) = get_package_version(session, &name) {
                versions.insert(name, version);
            } else {
                // Package not found
                return false;
            }
        }

        self.satisfied_by(&versions)
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<DebianDependency> for Relations {
    fn from(dep: DebianDependency) -> Self {
        dep.0
    }
}

impl From<Relations> for DebianDependency {
    fn from(rel: Relations) -> Self {
        DebianDependency(rel)
    }
}

pub trait TieBreaker {
    fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency>;
}

pub fn default_tie_breakers(session: &dyn Session) -> Vec<Box<dyn TieBreaker>> {
    let mut tie_breakers: Vec<Box<dyn TieBreaker>> = Vec::new();
    use crate::debian::build_deps::BuildDependencyTieBreaker;
    tie_breakers.push(Box::new(BuildDependencyTieBreaker::from_session(session)));

    #[cfg(feature = "udd")]
    {
        use crate::debian::udd::PopconTieBreaker;
        tie_breakers.push(Box::new(PopconTieBreaker));
    }

    tie_breakers
}

pub trait IntoDebianDependency: Dependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>>;
}

impl IntoDebianDependency for DebianDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        Some(vec![self.clone()])
    }
}

pub trait FromDebianDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>>;
}

pub fn extract_upstream_dependency(dep: &DebianDependency) -> Option<Box<dyn Dependency>> {
    crate::dependencies::RubyGemDependency::from_debian_dependency(dep)
        .or_else(|| {
            crate::dependencies::python::PythonPackageDependency::from_debian_dependency(dep)
        })
        .or_else(|| crate::dependencies::RubyGemDependency::from_debian_dependency(dep))
        .or_else(|| crate::dependencies::CargoCrateDependency::from_debian_dependency(dep))
        .or_else(|| crate::dependencies::python::PythonDependency::from_debian_dependency(dep))
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for DebianDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        let upstream_dep = extract_upstream_dependency(self)?;

        crate::upstream::find_upstream(upstream_dep.as_ref())
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::debian::UnsatisfiedAptDependencies
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(DebianDependency::new(&self.0)))
    }
}

pub fn extract_simple_exact_version(
    dep: &DebianDependency,
) -> Option<(String, Option<debversion::Version>)> {
    // Extract the package name and exact version from a dependency. Return None
    // if there are non-1 entries in the dependency, or non-1 relations in the entry or if the
    // version constraint is not Equal.
    let mut entries = dep.0.entries();
    let first_entry = entries.next()?;
    if entries.next().is_some() {
        return None;
    }

    let mut relations = first_entry.relations();
    let first_relation = relations.next()?;
    if relations.next().is_some() {
        return None;
    }

    let name = first_relation.name();
    let version = match first_relation.version() {
        Some((VersionConstraint::Equal, v)) => Some(v),
        None => None,
        _ => return None,
    };

    Some((name.to_string(), version))
}

pub fn extract_simple_min_version(
    dep: &DebianDependency,
) -> Option<(String, Option<debversion::Version>)> {
    // Extract the package name and minimum version from a dependency. Return None
    // if there are non-1 entries in the dependency, or non-1 relations in the entry or if the
    // version constraint is not GreaterThanEqual or absent.
    let mut entries = dep.0.entries();
    let first_entry = entries.next()?;
    if entries.next().is_some() {
        return None;
    }

    let mut relations = first_entry.relations();
    let first_relation = relations.next()?;
    if relations.next().is_some() {
        return None;
    }

    let name = first_relation.name();
    let version = match first_relation.version() {
        Some((VersionConstraint::GreaterThanEqual, v)) => Some(v),
        None => None,
        _ => return None,
    };

    Some((name.to_string(), version))
}

pub fn valid_debian_package_name(name: &str) -> bool {
    lazy_regex::regex_is_match!("[a-z0-9][a-z0-9+-\\.]+", name)
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum DebianDependencyCategory {
    Runtime,
    Build,
    Install,
    Test(String),
}

#[cfg(test)]
mod tests {
    use super::*;
    use maplit::hashset;

    #[test]
    fn test_valid_debian_package_name() {
        assert!(valid_debian_package_name("libssl-dev"));
        assert!(valid_debian_package_name("libssl1.1"));
        assert!(valid_debian_package_name("libssl1.1-dev"));
        assert!(valid_debian_package_name("libssl1.1-dev~foo"));
    }

    #[test]
    fn test_touches_package() {
        let dep = DebianDependency::new("libssl-dev");
        assert!(dep.touches_package("libssl-dev"));
        assert!(!dep.touches_package("libssl1.1"));
    }

    #[test]
    fn test_package_names() {
        let dep = DebianDependency::new("libssl-dev");
        assert_eq!(dep.package_names(), hashset! {"libssl-dev".to_string()});
    }

    #[test]
    fn test_package_names_multiple() {
        let dep = DebianDependency::new("libssl-dev, libssl1.1");
        assert_eq!(
            dep.package_names(),
            hashset! {"libssl-dev".to_string(), "libssl1.1".to_string()}
        );
    }

    #[test]
    fn test_package_names_multiple_with_version() {
        let dep = DebianDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        assert_eq!(
            dep.package_names(),
            hashset! {"libssl-dev".to_string(), "libssl1.1".to_string()}
        );
    }

    #[test]
    fn test_satisfied_by() {
        let dep = DebianDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        let mut versions = std::collections::HashMap::new();
        versions.insert("libssl-dev".to_string(), "1.2".parse().unwrap());
        versions.insert("libssl1.1".to_string(), "1.2".parse().unwrap());
        assert!(dep.satisfied_by(&versions));
    }

    #[test]
    fn test_satisfied_by_missing_package() {
        let dep = DebianDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        let mut versions = std::collections::HashMap::new();
        versions.insert("libssl-dev".to_string(), "1.2".parse().unwrap());
        assert!(!dep.satisfied_by(&versions));
    }

    #[test]
    fn test_satisfied_by_missing_version() {
        let dep = DebianDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        let mut versions = std::collections::HashMap::new();
        versions.insert("libssl-dev".to_string(), "1.2".parse().unwrap());
        versions.insert("libssl1.1".to_string(), "1.0".parse().unwrap());
        assert!(!dep.satisfied_by(&versions));
    }
}
