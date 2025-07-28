/// Trait for build system outputs.
///
/// This trait is implemented by types that represent outputs from a build system,
/// such as binary packages, library packages, etc.
pub trait Output: std::fmt::Debug {
    /// Get the family of this output (e.g., "binary", "python-package", etc.).
    ///
    /// # Returns
    /// A string identifying the output type family
    fn family(&self) -> &'static str;

    /// Get the dependencies declared by this output.
    ///
    /// # Returns
    /// A list of dependency names
    fn get_declared_dependencies(&self) -> Vec<String>;
}

#[derive(Debug)]
/// Output representing a binary executable.
pub struct BinaryOutput(pub String);

impl BinaryOutput {
    /// Create a new binary output.
    ///
    /// # Arguments
    /// * `name` - Name of the binary
    ///
    /// # Returns
    /// A new BinaryOutput instance
    pub fn new(name: &str) -> Self {
        BinaryOutput(name.to_owned())
    }
}

impl Output for BinaryOutput {
    fn family(&self) -> &'static str {
        "binary"
    }

    fn get_declared_dependencies(&self) -> Vec<String> {
        vec![]
    }
}

#[derive(Debug)]
/// Output representing a Python package.
pub struct PythonPackageOutput {
    /// Name of the Python package.
    pub name: String,
    /// Optional version of the Python package.
    pub version: Option<String>,
}

impl PythonPackageOutput {
    /// Create a new Python package output.
    ///
    /// # Arguments
    /// * `name` - Name of the Python package
    /// * `version` - Optional version of the Python package
    ///
    /// # Returns
    /// A new PythonPackageOutput instance
    pub fn new(name: &str, version: Option<&str>) -> Self {
        PythonPackageOutput {
            name: name.to_owned(),
            version: version.map(|s| s.to_owned()),
        }
    }
}

impl Output for PythonPackageOutput {
    fn family(&self) -> &'static str {
        "python-package"
    }

    fn get_declared_dependencies(&self) -> Vec<String> {
        vec![]
    }
}

#[derive(Debug)]
/// Output representing an R package.
pub struct RPackageOutput {
    /// Name of the R package.
    pub name: String,
}

impl RPackageOutput {
    /// Create a new R package output.
    ///
    /// # Arguments
    /// * `name` - Name of the R package
    ///
    /// # Returns
    /// A new RPackageOutput instance
    pub fn new(name: &str) -> Self {
        RPackageOutput {
            name: name.to_owned(),
        }
    }
}

impl Output for RPackageOutput {
    fn family(&self) -> &'static str {
        "r-package"
    }

    fn get_declared_dependencies(&self) -> Vec<String> {
        vec![]
    }
}
