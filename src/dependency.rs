use crate::session::Session;
use serde::{Deserialize, Serialize};

/// A dependency is a component that is required by a project to build or run.
pub trait Dependency: std::fmt::Debug {
    fn family(&self) -> &'static str;

    /// Check whether the dependency is present in the session.
    fn present(&self, session: &dyn Session) -> bool;

    /// Check whether the dependency is present in the project.
    fn project_present(&self, session: &dyn Session) -> bool;

    fn as_any(&self) -> &dyn std::any::Any;
}
