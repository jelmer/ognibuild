use crate::session::Session;

/// A dependency is a component that is required by a project to build or run.
pub trait Dependency: std::fmt::Debug {
    /// Get the family of this dependency (e.g., "apt", "pip", etc.).
    ///
    /// # Returns
    /// A string identifying the dependency type family
    fn family(&self) -> &'static str;

    /// Check whether the dependency is present in the session.
    fn present(&self, session: &dyn Session) -> bool;

    /// Check whether the dependency is present in the project.
    fn project_present(&self, session: &dyn Session) -> bool;

    /// Convert this dependency to Any for dynamic casting.
    ///
    /// This method allows for conversion of the dependency to concrete types at runtime.
    ///
    /// # Returns
    /// A reference to this dependency as Any
    fn as_any(&self) -> &dyn std::any::Any;
}

/// A dependency type that can be sent to a dep server as `[family, details]`.
pub struct DependencyFamily {
    family: &'static str,
    to_json: fn(&dyn Dependency) -> serde_json::Value,
    from_json: fn(serde_json::Value) -> Result<Box<dyn Dependency>, serde_json::Error>,
}

impl DependencyFamily {
    /// Register `T` under `family`, which must match `T::family`.
    pub const fn new<T>(family: &'static str) -> Self
    where
        T: Dependency + serde::Serialize + serde::de::DeserializeOwned + 'static,
    {
        Self {
            family,
            to_json: |dep| serde_json::to_value(dep.as_any().downcast_ref::<T>().unwrap()).unwrap(),
            from_json: |details| Ok(Box::new(serde_json::from_value::<T>(details)?)),
        }
    }
}

inventory::collect!(DependencyFamily);

fn find_family(family: &str) -> Option<&'static DependencyFamily> {
    inventory::iter::<DependencyFamily>
        .into_iter()
        .find(|f| f.family == family)
}

/// Serialize a dependency as `(family, details)`, or None if its family is not registered.
pub fn dependency_to_json(dep: &dyn Dependency) -> Option<(String, serde_json::Value)> {
    let family = find_family(dep.family())?;
    Some((family.family.to_string(), (family.to_json)(dep)))
}

/// Deserialize the details of a dependency, or None if its family is not registered.
pub fn dependency_from_json(
    family: &str,
    details: serde_json::Value,
) -> Result<Option<Box<dyn Dependency>>, serde_json::Error> {
    find_family(family)
        .map(|f| (f.from_json)(details))
        .transpose()
}
