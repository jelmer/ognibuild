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
