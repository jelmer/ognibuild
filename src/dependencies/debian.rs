use crate::dependency::Dependency;
use crate::session::Session;
use debian_control::lossless::relations::{Entry, Relation, Relations};
use debian_control::relations::VersionConstraint;
use debversion::Version;
use serde::{Deserialize, Deserializer, Serialize, Serializer};
use std::collections::HashSet;
use std::hash::Hash;

/// Represents a Debian dependency.
pub struct DebianDependency(Relations);

impl std::fmt::Debug for DebianDependency {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("DebianDependency")
            .field(&self.0.to_string())
            .finish()
    }
}

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

    /// Iterate over the entries in the dependency.
    pub fn iter(&self) -> impl Iterator<Item = Entry> + '_ {
        self.0.entries()
    }

    /// Get the relations of the dependency.
    pub fn relation_string(&self) -> String {
        self.0.to_string()
    }

    /// Create a new dependency from a package name with a specific version.
    pub fn simple(name: &str) -> DebianDependency {
        Self::new(name)
    }

    /// Check if the dependency is empty.
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }

    /// Create a new dependency with a minimum version.
    pub fn new_with_min_version(name: &str, min_version: &Version) -> DebianDependency {
        Self::new_with_version(name, VersionConstraint::GreaterThanEqual, min_version)
    }

    /// Create a new dependency with a version constraint.
    pub fn new_with_version(
        name: &str,
        constraint: VersionConstraint,
        version: &Version,
    ) -> DebianDependency {
        DebianDependency(
            format!("{} ({} {})", name, constraint, version)
                .parse()
                .unwrap_or_else(|_| {
                    panic!(
                        "Failed to parse dependency: {} ({} {})",
                        name, constraint, version
                    )
                }),
        )
    }

    /// Check if the dependency touches a specific package.
    pub fn touches_package(&self, package: &str) -> bool {
        for entry in self.0.entries() {
            for relation in entry.relations() {
                if relation.try_name().as_deref() == Some(package) {
                    return true;
                }
            }
        }
        false
    }

    /// Get the package names from the dependency.
    pub fn package_names(&self) -> HashSet<String> {
        let mut names = HashSet::new();
        for entry in self.0.entries() {
            for relation in entry.relations() {
                if let Some(name) = relation.try_name() {
                    names.insert(name);
                }
            }
        }
        names
    }

    /// Check if the dependency is satisfied by the given versions.
    pub fn satisfied_by(
        &self,
        versions: &std::collections::HashMap<String, debversion::Version>,
    ) -> bool {
        let relation_satisfied = |relation: Relation| -> bool {
            let Some(name) = relation.try_name() else {
                return false;
            };
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
    let argv = vec!["dpkg-query", "-W", "-f=${Version}\n", package];
    let output = session
        .command(argv)
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    match output.status.code() {
        Some(0) => {
            let output = String::from_utf8(output.stdout).unwrap();
            if output.trim().is_empty() {
                return None;
            }
            Some(output.trim().parse().unwrap())
        }
        Some(1) => None,
        _ => panic!("Failed to run dpkg-query"),
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

        let result = self.satisfied_by(&versions);
        if !result {
            log::debug!("Dependency not satisfied: {:?}", self);
        } else {
            log::debug!("Dependency satisfied: {:?}", self);
        }
        result
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

/// Trait for breaking ties between multiple dependencies.
pub trait TieBreaker {
    /// Break ties between multiple dependencies.
    fn break_tie<'a>(&self, reqs: &[&'a DebianDependency]) -> Option<&'a DebianDependency>;
}

/// Default tie breakers for Debian dependencies.
pub fn default_tie_breakers(session: &dyn Session) -> Vec<Box<dyn TieBreaker>> {
    let mut tie_breakers: Vec<Box<dyn TieBreaker>> = Vec::new();
    use crate::debian::build_deps::BuildDependencyTieBreaker;
    match BuildDependencyTieBreaker::try_from_session(session) {
        Ok(tie_breaker) => {
            tie_breakers.push(Box::new(tie_breaker));
        }
        Err(e) => {
            log::warn!(
                "Failed to create BuildDependencyTieBreaker: {}. Continuing without it.",
                e
            );
        }
    }

    #[cfg(feature = "udd")]
    {
        use crate::debian::udd::PopconTieBreaker;
        tie_breakers.push(Box::new(PopconTieBreaker));
    }

    tie_breakers
}

/// Trait for converting a dependency into a DebianDependency.
pub trait IntoDebianDependency: Dependency {
    /// Convert a dependency into a DebianDependency.
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

/// Trait for converting a DebianDependency into an upstream dependency.
pub trait FromDebianDependency {
    /// Convert a DebianDependency into an upstream dependency.
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>>;
}

/// Extract an upstream dependency from a DebianDependency.
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

/// Extract the package name and exact version from a dependency.
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

    let name = first_relation.try_name()?;
    let version = match first_relation.version() {
        Some((VersionConstraint::Equal, v)) => Some(v),
        None => None,
        _ => return None,
    };

    Some((name.to_string(), version))
}

/// Extract the package name and minimum version from a dependency.
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

    let name = first_relation.try_name()?;
    let version = match first_relation.version() {
        Some((VersionConstraint::GreaterThanEqual, v)) => Some(v),
        None => None,
        _ => return None,
    };

    Some((name.to_string(), version))
}

/// Check if a string is a valid Debian package name.
pub fn valid_debian_package_name(name: &str) -> bool {
    lazy_regex::regex_is_match!("[a-z0-9][a-z0-9+-\\.]+", name)
}

/// Map an automake-versioned command to the unversioned binary apt ships.
///
/// A maintainer-mode autotools rebuild reruns the exact automake series baked
/// into the shipped Makefile, e.g. `aclocal-1.16`/`automake-1.16`. apt only
/// ships the unversioned `aclocal`/`automake` (plus whatever series is
/// current), so a path search for `aclocal-1.16` finds nothing while the
/// unversioned name resolves to the automake package. Returns `None` for any
/// other command.
pub fn unversioned_automake_binary(binary_name: &str) -> Option<&'static str> {
    let (stem, version) = binary_name.split_once('-')?;
    if version.is_empty() || !version.chars().all(|c| c.is_ascii_digit() || c == '.') {
        return None;
    }
    match stem {
        "aclocal" => Some("aclocal"),
        "automake" => Some("automake"),
        _ => None,
    }
}

const BIN_PATHS: &[&str] = &["/usr/bin", "/bin"];

impl IntoDebianDependency for super::BinaryDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        // apt does not ship the version-stamped automake binaries a
        // maintainer-mode rebuild asks for, so look them up unversioned.
        let binary_name =
            unversioned_automake_binary(self.binary_name()).unwrap_or(self.binary_name());

        let paths = if std::path::Path::new(binary_name).is_absolute() {
            vec![binary_name.to_string()]
        } else {
            BIN_PATHS
                .iter()
                .map(|p| format!("{}/{}", p, binary_name))
                .collect()
        };
        // TODO(jelmer): Check for binaries which use alternatives
        Some(
            apt.get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), false, false)
                .unwrap()
                .iter()
                .map(|p| DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
/// Represents the category of a Debian dependency.
pub enum DebianDependencyCategory {
    /// A runtime dependency.
    Runtime,

    /// A build dependency.
    Build,

    /// A runtime dependency that is also a build dependency.
    Install,

    /// A test dependency.
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
    fn test_unversioned_automake_binary() {
        assert_eq!(unversioned_automake_binary("aclocal-1.16"), Some("aclocal"));
        assert_eq!(
            unversioned_automake_binary("automake-1.16"),
            Some("automake")
        );
        // Already unversioned, or unrelated commands, are left alone.
        assert_eq!(unversioned_automake_binary("aclocal"), None);
        assert_eq!(unversioned_automake_binary("autoconf-2.71"), None);
        assert_eq!(unversioned_automake_binary("gcc-13"), None);
        assert_eq!(unversioned_automake_binary("automake-foreign"), None);
    }

    #[test]
    fn test_touches_package() {
        let dep = DebianDependency::new("libssl-dev");
        assert!(dep.touches_package("libssl-dev"));
        assert!(!dep.touches_package("libssl1.1"));
    }

    #[test]
    fn test_new_with_version() {
        let version: Version = "1.7-5".parse().unwrap();
        let dep =
            DebianDependency::new_with_version("r-cran-matrix", VersionConstraint::Equal, &version);
        assert_eq!(dep.relation_string(), "r-cran-matrix (= 1.7-5)");

        let dep = DebianDependency::new_with_version(
            "r-cran-matrix",
            VersionConstraint::GreaterThan,
            &version,
        );
        assert_eq!(dep.relation_string(), "r-cran-matrix (>> 1.7-5)");
    }

    #[test]
    fn test_new_with_min_version() {
        let version: Version = "1.7-5".parse().unwrap();
        let dep = DebianDependency::new_with_min_version("r-cran-matrix", &version);
        assert_eq!(dep.relation_string(), "r-cran-matrix (>= 1.7-5)");
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
