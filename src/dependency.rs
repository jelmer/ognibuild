use crate::session::Session;

/// Why a check could not tell whether a dependency is present.
#[derive(Debug)]
pub enum Error {
    /// The command that does the check could not be run.
    Session(crate::session::Error),
    /// The command that does the check failed, or gave output that cannot be
    /// understood.
    Command {
        /// The command, such as `dpkg-query`.
        command: String,
        /// What went wrong.
        message: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Session(e) => write!(f, "{}", e),
            Error::Command { command, message } => write!(f, "{}: {}", command, message),
        }
    }
}

impl std::error::Error for Error {}

impl From<crate::session::Error> for Error {
    fn from(e: crate::session::Error) -> Self {
        Error::Session(e)
    }
}

/// A dependency is a component that is required by a project to build or run.
pub trait Dependency: std::fmt::Debug {
    /// Get the family of this dependency (e.g., "apt", "pip", etc.).
    ///
    /// # Returns
    /// A string identifying the dependency type family
    fn family(&self) -> &'static str;

    /// Check whether the dependency is present in the session.
    ///
    /// An error means the check could not tell whether it is present.
    fn present(&self, session: &dyn Session) -> Result<bool, Error>;

    /// Check whether the dependency is present in the project.
    fn project_present(&self, session: &dyn Session) -> Result<bool, Error>;

    /// Convert this dependency to Any for dynamic casting.
    ///
    /// This method allows for conversion of the dependency to concrete types at runtime.
    ///
    /// # Returns
    /// A reference to this dependency as Any
    fn as_any(&self) -> &dyn std::any::Any;
}
