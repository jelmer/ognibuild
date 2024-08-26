/// The category of a dependency
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum DependencyCategory {
    /// A dependency that is required for the package to build
    Universal,
    /// Building of artefacts
    Build,
    /// For running artefacts after build or install 
    Runtime,
    /// Test infrastructure, e.g. test frameworks or test runners
    Test,
    /// Needed for development, e.g. linters or IDE plugins
    Dev,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Action {
    Clean,
    Build,
    Test,
    Install
}
