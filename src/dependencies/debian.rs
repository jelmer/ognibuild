use debian_control::relations::Relations;
use std::collections::HashSet;
use std::str::FromStr;

pub struct AptDependency(Relations);

impl AptDependency {
    /// Create a new dependency from a package name.
    pub fn new(name: &str) -> AptDependency {
        AptDependency(
            name.parse()
                .unwrap_or_else(|_| panic!("Failed to parse dependency: {}", name)),
        )
    }

    pub fn new_with_min_version(name: &str, min_version: &str) -> AptDependency {
        AptDependency(
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

impl From<AptDependency> for Relations {
    fn from(dep: AptDependency) -> Self {
        dep.0
    }
}

impl From<Relations> for AptDependency {
    fn from(rel: Relations) -> Self {
        AptDependency(rel)
    }
}

#[cfg(test)]
mod tests {
    use maplit::hashset;
    use super::*;

    #[test]
    fn test_touches_package() {
        let dep = AptDependency::new("libssl-dev");
        assert!(dep.touches_package("libssl-dev"));
        assert!(!dep.touches_package("libssl1.1"));
    }

    #[test]
    fn test_package_names() {
        let dep = AptDependency::new("libssl-dev");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string()});
    }

    #[test]
    fn test_package_names_multiple() {
        let dep = AptDependency::new("libssl-dev, libssl1.1");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string(), "libssl1.1".to_string()});
    }

    #[test]
    fn test_package_names_multiple_with_version() {
        let dep = AptDependency::new("libssl-dev (>= 1.1), libssl1.1 (>= 1.1)");
        assert_eq!(dep.package_names(), hashset!{"libssl-dev".to_string(), "libssl1.1".to_string()});
    }
}
