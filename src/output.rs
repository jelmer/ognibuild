pub trait Output: std::fmt::Debug {
    fn family(&self) -> &'static str;

    fn get_declared_dependencies(&self) -> Vec<String>;
}

#[derive(Debug)]
pub struct BinaryOutput(pub String);

impl BinaryOutput {
    pub fn new(name: &str) -> Self {
        BinaryOutput(name.to_string())
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
pub struct PythonPackageOutput {
    pub name: String,
    pub version: Option<String>,
}

impl PythonPackageOutput {
    pub fn new(name: &str, version: Option<&str>) -> Self {
        PythonPackageOutput { name: name.to_string(), version: version.map(|s| s.to_string()) }
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
pub struct RPackageOutput {
    pub name: String,
}

impl RPackageOutput {
    pub fn new(name: &str) -> Self {
        RPackageOutput { name: name.to_string() }
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
