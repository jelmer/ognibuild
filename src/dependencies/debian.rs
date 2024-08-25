use debian_control::relations::Relations;
use std::collections::HashSet;
use std::str::FromStr;
use std::hash::{Hash, Hasher};

#[derive(Debug)]
pub struct DebianDependency(Relations);

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

    pub fn relation_string(&self) -> String {
        self.0.to_string()
    }

    pub fn new_with_min_version(name: &str, min_version: &str) -> DebianDependency {
        DebianDependency(
            format!("{} (>= {})", name, min_version).parse().unwrap_or_else(|_| panic!("Failed to parse dependency: {} (>= {})",
                name, min_version)),
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
    fn break_tie<'a>(&mut self, reqs: Vec<&'a DebianDependency>) -> Option<&'a DebianDependency>;
}

#[cfg(test)]
mod tests {
    use maplit::hashset;
    use super::*;

    #[test]
    fn test_touches_package() {
        let dep = DebianDependency::new("libssl-dev");
        assert!(dep.touches_package("libssl-dev"));
        assert!(!dep.touches_package("libssl1.1"));
    }

    #[test]
    fn test_package_names() {
        let dep = DebianDependency::new("libssl-dev");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string()});
    }

    #[test]
    fn test_package_names_multiple() {
        let dep = DebianDependency::new("libssl-dev, libssl1.1");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string(), "libssl1.1".to_string()});
    }

    #[test]
    fn test_package_names_multiple_with_version() {
        let dep = DebianDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string(), "libssl1.1".to_string()});
    }
}
