//! This module provides a trait for dependencies that can find their upstream metadata.
use crate::dependency::Dependency;
use lazy_static::lazy_static;
use std::sync::RwLock;
pub use upstream_ontologist::UpstreamMetadata;

/// Type alias for custom upstream metadata providers.
pub type UpstreamProvider = Box<dyn Fn(&dyn Dependency) -> Option<UpstreamMetadata> + Send + Sync>;

lazy_static! {
    /// Global registry of custom upstream metadata providers.
    static ref CUSTOM_PROVIDERS: RwLock<Vec<UpstreamProvider>> = RwLock::new(Vec::new());
}

/// Register a custom upstream metadata provider.
///
/// Custom providers are checked before built-in providers when finding upstream metadata.
///
/// # Arguments
/// * `provider` - A function that takes a dependency and returns optional upstream metadata
///
/// # Example
/// ```no_run
/// use ognibuild::upstream::{register_upstream_provider, UpstreamMetadata};
/// use ognibuild::dependency::Dependency;
///
/// register_upstream_provider(|dep| {
///     if dep.family() == "custom" {
///         Some(UpstreamMetadata::default())
///     } else {
///         None
///     }
/// });
/// ```
pub fn register_upstream_provider<F>(provider: F)
where
    F: Fn(&dyn Dependency) -> Option<UpstreamMetadata> + Send + Sync + 'static,
{
    CUSTOM_PROVIDERS.write().unwrap().push(Box::new(provider));
}

/// Clear all registered custom upstream providers.
///
/// This is useful for testing to ensure a clean state between tests.
pub fn clear_custom_providers() {
    CUSTOM_PROVIDERS.write().unwrap().clear();
}

/// A trait for dependencies that can find their upstream metadata.
pub trait FindUpstream: Dependency {
    /// Find the upstream metadata for this dependency.
    fn find_upstream(&self) -> Option<UpstreamMetadata>;
}

/// Find the upstream metadata for a dependency.
///
/// This function attempts to find upstream metadata (like repository URL,
/// homepage, etc.) for the given dependency. It first checks any registered
/// custom providers, then falls back to trying to downcast the dependency to
/// various concrete dependency types that implement the FindUpstream trait.
///
/// # Arguments
/// * `dependency` - The dependency to find upstream metadata for
///
/// # Returns
/// * `Some(UpstreamMetadata)` if upstream metadata was found
/// * `None` if no upstream metadata could be found
pub fn find_upstream(dependency: &dyn Dependency) -> Option<UpstreamMetadata> {
    // First try custom providers
    for provider in CUSTOM_PROVIDERS.read().unwrap().iter() {
        if let Some(metadata) = provider(dependency) {
            return Some(metadata);
        }
    }
    #[cfg(feature = "debian")]
    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::debian::DebianDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::python::PythonPackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::RubyGemDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::node::NodePackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::CargoCrateDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::go::GoPackageDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::perl::PerlModuleDependency>()
    {
        return dep.find_upstream();
    }

    if let Some(dep) = dependency
        .as_any()
        .downcast_ref::<crate::dependencies::haskell::HaskellPackageDependency>()
    {
        return dep.find_upstream();
    }

    None
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[derive(Debug)]
    struct TestDependency {
        #[allow(dead_code)]
        name: String,
    }

    impl Dependency for TestDependency {
        fn family(&self) -> &'static str {
            "test"
        }

        fn present(&self, _session: &dyn crate::session::Session) -> bool {
            false
        }

        fn project_present(&self, _session: &dyn crate::session::Session) -> bool {
            false
        }

        fn as_any(&self) -> &dyn Any {
            self
        }
    }

    #[test]
    fn test_register_custom_provider() {
        // Clear any existing providers from other tests
        clear_custom_providers();

        let test_dep = TestDependency {
            name: "test-package".to_string(),
        };

        // Initially, no upstream metadata should be found
        let initial_result = find_upstream(&test_dep);
        assert!(
            initial_result.is_none(),
            "Expected no metadata initially, but found: {:?}",
            initial_result
        );

        // Register a custom provider
        register_upstream_provider(|dep| {
            if dep.family() == "test" {
                let mut metadata = UpstreamMetadata::default();
                metadata.insert(upstream_ontologist::UpstreamDatumWithMetadata {
                    datum: upstream_ontologist::UpstreamDatum::Repository(
                        "https://github.com/test/repo".to_string(),
                    ),
                    certainty: Some(upstream_ontologist::Certainty::Certain),
                    origin: None,
                });
                Some(metadata)
            } else {
                None
            }
        });

        // Now upstream metadata should be found via the custom provider
        let metadata = find_upstream(&test_dep).unwrap();
        assert_eq!(metadata.repository(), Some("https://github.com/test/repo"));

        // Clean up
        clear_custom_providers();
    }

    #[test]
    fn test_multiple_custom_providers() {
        // Clear any existing providers from other tests
        clear_custom_providers();

        let test_dep = TestDependency {
            name: "special-package".to_string(),
        };

        // Register first provider (doesn't match)
        register_upstream_provider(|dep| {
            if dep.family() == "other" {
                let mut metadata = UpstreamMetadata::default();
                metadata.insert(upstream_ontologist::UpstreamDatumWithMetadata {
                    datum: upstream_ontologist::UpstreamDatum::Repository(
                        "https://example.com/wrong".to_string(),
                    ),
                    certainty: Some(upstream_ontologist::Certainty::Certain),
                    origin: None,
                });
                Some(metadata)
            } else {
                None
            }
        });

        // Register second provider (matches)
        register_upstream_provider(|dep| {
            if dep.family() == "test" {
                let mut metadata = UpstreamMetadata::default();
                metadata.insert(upstream_ontologist::UpstreamDatumWithMetadata {
                    datum: upstream_ontologist::UpstreamDatum::Repository(
                        "https://example.com/correct".to_string(),
                    ),
                    certainty: Some(upstream_ontologist::Certainty::Certain),
                    origin: None,
                });
                Some(metadata)
            } else {
                None
            }
        });

        // Should find metadata from the second provider
        let metadata = find_upstream(&test_dep).unwrap();
        assert_eq!(metadata.repository(), Some("https://example.com/correct"));

        clear_custom_providers();
    }

    #[test]
    fn test_clear_custom_providers() {
        clear_custom_providers();

        let test_dep = TestDependency {
            name: "test-package".to_string(),
        };

        // Register a provider
        register_upstream_provider(|dep| {
            if dep.family() == "test" {
                let mut metadata = UpstreamMetadata::default();
                metadata.insert(upstream_ontologist::UpstreamDatumWithMetadata {
                    datum: upstream_ontologist::UpstreamDatum::Homepage(
                        "https://example.com".to_string(),
                    ),
                    certainty: Some(upstream_ontologist::Certainty::Certain),
                    origin: None,
                });
                Some(metadata)
            } else {
                None
            }
        });

        // Verify it works
        assert!(find_upstream(&test_dep).is_some());

        // Clear providers
        clear_custom_providers();

        // Verify provider is gone
        assert!(find_upstream(&test_dep).is_none());
    }
}
