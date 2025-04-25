use crate::buildlog::ToDependency;
use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

/// Dependency handling for autoconf-based projects.
pub mod autoconf;
#[cfg(feature = "debian")]
/// Dependency handling for Debian packages.
pub mod debian;
/// Dependency handling for Go projects.
pub mod go;
/// Dependency handling for Haskell projects.
pub mod haskell;
/// Dependency handling for Java projects.
pub mod java;
/// Dependency handling for LaTeX projects.
pub mod latex;
/// Dependency handling for Node.js projects.
pub mod node;
/// Dependency handling for GNU Octave projects.
pub mod octave;
/// Dependency handling for Perl projects.
pub mod perl;
/// Dependency handling for PHP projects.
pub mod php;
/// Dependency handling for pytest-specific dependencies.
pub mod pytest;
/// Dependency handling for Python projects.
pub mod python;
/// Dependency handling for R projects.
pub mod r;
/// Dependency handling for vague or generic dependencies.
pub mod vague;
/// Dependency handling for XML-related requirements.
pub mod xml;

/// Dependency on a system binary or executable file.
///
/// This represents a dependency on an executable binary command that
/// must be available in the PATH.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDependency {
    /// Name of the binary executable
    binary_name: String,
}

impl BinaryDependency {
    /// Create a new BinaryDependency.
    ///
    /// # Arguments
    /// * `binary_name` - Name of the binary executable
    ///
    /// # Returns
    /// A new BinaryDependency instance
    pub fn new(binary_name: &str) -> Self {
        Self {
            binary_name: binary_name.to_string(),
        }
    }
}

impl Dependency for BinaryDependency {
    fn family(&self) -> &'static str {
        "binary"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["which", &self.binary_name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCommand {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(BinaryDependency::new(&self.0)))
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCommandOrBuildFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(BinaryDependency::new(&self.filename)))
    }
}

const BIN_PATHS: &[&str] = &["/usr/bin", "/bin"];

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for BinaryDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = if std::path::Path::new(&self.binary_name).is_absolute() {
            vec![self.binary_name.clone()]
        } else {
            BIN_PATHS
                .iter()
                .map(|p| format!("{}/{}", p, self.binary_name))
                .collect()
        };
        // TODO(jelmer): Check for binaries which use alternatives
        Some(
            apt.get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), false, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

/// Dependency for accessing VCS control directories.
///
/// This represents a dependency on access to VCS control directories
/// like .git, .svn, etc., which might be needed by certain build processes.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsControlDirectoryAccessDependency {
    /// List of affected version control systems (e.g., "git", "svn")
    pub vcs: Vec<String>,
}

impl VcsControlDirectoryAccessDependency {
    /// Create a new VcsControlDirectoryAccessDependency.
    ///
    /// # Arguments
    /// * `vcs` - List of version control systems
    ///
    /// # Returns
    /// A new VcsControlDirectoryAccessDependency instance
    pub fn new(vcs: Vec<&str>) -> Self {
        Self {
            vcs: vcs.iter().map(|s| s.to_string()).collect(),
        }
    }
}

impl Dependency for VcsControlDirectoryAccessDependency {
    fn family(&self) -> &'static str {
        "vcs-access"
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        self.vcs.iter().all(|vcs| match vcs.as_str() {
            "git" => session
                .command(vec!["git", "rev-parse", "--is-inside-work-tree"])
                .stdout(std::process::Stdio::null())
                .stderr(std::process::Stdio::null())
                .run()
                .unwrap()
                .success(),
            _ => todo!(),
        })
    }

    fn present(&self, _session: &dyn Session) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for VcsControlDirectoryAccessDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let pkgs = self
            .vcs
            .iter()
            .filter_map(|vcs| match vcs.as_str() {
                "git" => Some("git"),
                "hg" => Some("mercurial"),
                "svn" => Some("subversion"),
                "bzr" => Some("bzr"),
                _ => {
                    log::warn!("Unknown VCS {}", vcs);
                    None
                }
            })
            .collect::<Vec<_>>();

        let rels: Vec<debian_control::lossless::relations::Relations> =
            pkgs.iter().map(|p| p.parse().unwrap()).collect();

        Some(
            rels.into_iter()
                .map(|p| crate::dependencies::debian::DebianDependency::from(p))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::VcsControlDirectoryNeeded {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(VcsControlDirectoryAccessDependency::new(
            self.vcs.iter().map(|s| s.as_str()).collect(),
        )))
    }
}

/// Dependency on a Lua module.
///
/// This represents a dependency on a Lua module that can be loaded
/// with the require() function.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuaModuleDependency {
    /// Name of the Lua module
    module: String,
}

impl LuaModuleDependency {
    /// Create a new LuaModuleDependency.
    ///
    /// # Arguments
    /// * `module` - Name of the Lua module
    ///
    /// # Returns
    /// A new LuaModuleDependency instance
    pub fn new(module: &str) -> Self {
        Self {
            module: module.to_string(),
        }
    }
}

impl Dependency for LuaModuleDependency {
    fn family(&self) -> &'static str {
        "lua-module"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // lua -e 'package_name = "socket"; status, _ = pcall(require, package_name); if status then os.exit(0) else os.exit(1) end'
        session
            .command(vec![
                "lua",
                "-e",
                &format!(
                    r#"package_name = "{}"; status, _ = pcall(require, package_name); if status then os.exit(0) else os.exit(1) end"#,
                    self.module
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingLuaModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(LuaModuleDependency::new(&self.0)))
    }
}

/// Dependency on a Rust crate from crates.io.
///
/// This represents a dependency on a Rust crate that can be fetched
/// from the crates.io registry.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoCrateDependency {
    /// Name of the crate
    pub name: String,
    /// Optional list of required features
    pub features: Option<Vec<String>>,
    /// Optional API version requirement
    pub api_version: Option<String>,
    /// Optional minimum version requirement
    pub minimum_version: Option<String>,
}

impl CargoCrateDependency {
    /// Create a new CargoCrateDependency with features and API version.
    ///
    /// # Arguments
    /// * `name` - Name of the crate
    /// * `features` - Optional list of required features
    /// * `api_version` - Optional API version requirement
    ///
    /// # Returns
    /// A new CargoCrateDependency instance
    pub fn new(name: &str, features: Option<Vec<&str>>, api_version: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            features: features.map(|v| v.iter().map(|s| s.to_string()).collect()),
            api_version: api_version.map(|s| s.to_string()),
            minimum_version: None,
        }
    }

    /// Create a new CargoCrateDependency with just a name.
    ///
    /// # Arguments
    /// * `name` - Name of the crate
    ///
    /// # Returns
    /// A new CargoCrateDependency instance without features or version requirements
    pub fn simple(name: &str) -> Self {
        Self {
            name: name.to_string(),
            features: None,
            api_version: None,
            minimum_version: None,
        }
    }
}

impl Dependency for CargoCrateDependency {
    fn family(&self) -> &'static str {
        "cargo-crate"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["cargo".to_string(), "metadata".to_string()];
        if let Some(api_version) = &self.api_version {
            cmd.push(format!("--version={}", api_version));
        }
        let output = session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::null())
            .output()
            .unwrap();
        let metadata: serde_json::Value = serde_json::from_slice(&output.stdout).unwrap();
        let packages = metadata["packages"].as_array().unwrap();
        packages.iter().any(|package| {
            package["name"] == self.name
                && (self.features.is_none()
                    || package["features"].as_array().unwrap().iter().all(|f| {
                        self.features
                            .as_ref()
                            .unwrap()
                            .contains(&f.as_str().unwrap().to_string())
                    }))
        })
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for CargoCrateDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let path = format!(
            "/usr/share/cargo/registry/{}\\-[0-9]+.*/Cargo\\.toml",
            self.name
        );

        Some(
            apt.get_packages_for_paths(vec![&path], true, false)
                .unwrap()
                .iter()
                .map(|p| {
                    if self.api_version.is_some() {
                        crate::dependencies::debian::DebianDependency::new_with_min_version(
                            p.as_str(),
                            &self.api_version.as_ref().unwrap().parse().unwrap(),
                        )
                    } else {
                        crate::dependencies::debian::DebianDependency::simple(p.as_str())
                    }
                })
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCargoCrate {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(CargoCrateDependency::new(
            &self.crate_name,
            None,
            None,
        )))
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::FromDebianDependency for CargoCrateDependency {
    fn from_debian_dependency(
        dependency: &crate::dependencies::debian::DebianDependency,
    ) -> Option<Box<dyn Dependency>> {
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;
        let (_, name, api_version, features) =
            lazy_regex::regex_captures!(r"librust-(.*)-([^-+]+)(\+.*?)-dev", &name)?;

        let features = if features.is_empty() {
            HashSet::new()
        } else {
            features[1..].split("-").collect::<HashSet<_>>()
        };

        Some(Box::new(Self {
            name: name.to_string(),
            api_version: Some(api_version.to_string()),
            features: Some(features.into_iter().map(|t| t.to_string()).collect()),
            minimum_version: min_version.map(|v| v.upstream_version),
        }))
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingRustCompiler {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(BinaryDependency::new("rustc")))
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for CargoCrateDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(upstream_ontologist::providers::rust::remote_crate_data(
            &self.name,
        ))
        .ok()
    }
}

/// Dependency on a pkg-config module.
///
/// This represents a dependency on a library that can be found
/// and configured using the pkg-config system.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgConfigDependency {
    /// Name of the pkg-config module
    module: String,
    /// Optional minimum version requirement
    minimum_version: Option<String>,
}

impl PkgConfigDependency {
    /// Create a new PkgConfigDependency with a version requirement.
    ///
    /// # Arguments
    /// * `module` - Name of the pkg-config module
    /// * `minimum_version` - Optional minimum version requirement
    ///
    /// # Returns
    /// A new PkgConfigDependency instance
    pub fn new(module: &str, minimum_version: Option<&str>) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    /// Create a new PkgConfigDependency without a version requirement.
    ///
    /// # Arguments
    /// * `module` - Name of the pkg-config module
    ///
    /// # Returns
    /// A new PkgConfigDependency instance without a version requirement
    pub fn simple(module: &str) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: None,
        }
    }
}

impl Dependency for PkgConfigDependency {
    fn family(&self) -> &'static str {
        "pkg-config"
    }

    fn present(&self, session: &dyn Session) -> bool {
        log::debug!("Checking for pkg-config module {}", self.module);
        let cmd = [
            "pkg-config".to_string(),
            "--exists".to_string(),
            if let Some(minimum_version) = &self.minimum_version {
                format!("{} >= {}", self.module, minimum_version)
            } else {
                self.module.clone()
            },
        ];
        let result = session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success();
        if !result {
            log::debug!("pkg-config module {} not found", self.module);
        } else {
            log::debug!("pkg-config module {} found", self.module);
        }
        result
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PkgConfigDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let mut names = apt
            .get_packages_for_paths(
                [format!(
                    "/usr/lib/.*/pkgconfig/{}\\.pc",
                    regex::escape(&self.module)
                )]
                .iter()
                .map(|s| s.as_str())
                .collect(),
                true,
                false,
            )
            .unwrap();

        if names.is_empty() {
            names = apt
                .get_packages_for_paths(
                    [
                        format!("/usr/lib/pkgconfig/{}\\.pc", regex::escape(&self.module)),
                        format!("/usr/share/pkgconfig/{}\\.pc", regex::escape(&self.module)),
                    ]
                    .iter()
                    .map(|s| s.as_str())
                    .collect(),
                    true,
                    false,
                )
                .unwrap();
        }

        if names.is_empty() {
            return None;
        }

        Some(if let Some(minimum_version) = &self.minimum_version {
            let minimum_version: debversion::Version = minimum_version.parse().unwrap();
            names
                .into_iter()
                .map(|name| {
                    crate::dependencies::debian::DebianDependency::new_with_min_version(
                        &name,
                        &minimum_version,
                    )
                })
                .collect()
        } else {
            names
                .into_iter()
                .map(|name| crate::dependencies::debian::DebianDependency::simple(&name))
                .collect()
        })
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingPkgConfig {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PkgConfigDependency::new(
            &self.module,
            self.minimum_version.as_ref().map(|s| s.as_str()),
        )))
    }
}

/// Dependency on a file or directory at a specific path.
///
/// This represents a dependency on a file or directory that must
/// exist at a specific path in the filesystem.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDependency {
    /// Path to the required file or directory
    path: PathBuf,
}

impl From<PathBuf> for PathDependency {
    fn from(path: PathBuf) -> Self {
        Self { path }
    }
}

impl PathDependency {
    /// Create a new PathDependency.
    ///
    /// # Arguments
    /// * `path` - Path to the required file or directory
    ///
    /// # Returns
    /// A new PathDependency instance
    pub fn new(path: &str) -> Self {
        Self {
            path: PathBuf::from(path),
        }
    }
}

impl Dependency for PathDependency {
    fn family(&self) -> &'static str {
        "path"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        self.path.exists()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        if self.path.is_absolute() {
            false
        } else {
            self.path.exists()
        }
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PathDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(
            apt.get_packages_for_paths(vec![self.path.to_str().unwrap()], false, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PathDependency {
            path: PathBuf::from(&self.path),
        }))
    }
}

/// Dependency on a C header file.
///
/// This represents a dependency on a C header file that must
/// be available in the system include paths.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CHeaderDependency {
    /// Name of the C header file
    header: String,
}

impl CHeaderDependency {
    /// Create a new CHeaderDependency.
    ///
    /// # Arguments
    /// * `header` - Name of the C header file
    ///
    /// # Returns
    /// A new CHeaderDependency instance
    pub fn new(header: &str) -> Self {
        Self {
            header: header.to_string(),
        }
    }
}

impl Dependency for CHeaderDependency {
    fn family(&self) -> &'static str {
        "c-header"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec![
                "sh",
                "-c",
                &format!("echo \"#include <{}>\" | cc -E -", self.header),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for CHeaderDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let mut deps = apt
            .get_packages_for_paths(
                vec![std::path::Path::new("/usr/include")
                    .join(&self.header)
                    .to_str()
                    .unwrap()],
                false,
                false,
            )
            .unwrap();
        if deps.is_empty() {
            deps = apt
                .get_packages_for_paths(
                    vec![std::path::Path::new("/usr/include")
                        .join(".*")
                        .join(&self.header)
                        .to_str()
                        .unwrap()],
                    true,
                    false,
                )
                .unwrap();
        }
        if deps.is_empty() {
            return None;
        }
        Some(
            deps.into_iter()
                .map(|name| crate::dependencies::debian::DebianDependency::simple(&name))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCHeader {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(CHeaderDependency::new(&self.header)))
    }
}

/// Dependency on a JavaScript runtime environment.
///
/// This represents a dependency on a JavaScript runtime environment
/// like Node.js or a web browser.
#[derive(Debug, Clone)]
pub struct JavaScriptRuntimeDependency;

impl Dependency for JavaScriptRuntimeDependency {
    fn family(&self) -> &'static str {
        "javascript-runtime"
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["node", "-e", "process.exit(0)"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for JavaScriptRuntimeDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = vec!["/usr/bin/node", "/usr/bin/duk"];
        Some(
            apt.get_packages_for_paths(paths, false, false)
                .map(|p| {
                    p.iter()
                        .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                        .collect()
                })
                .unwrap(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingJavaScriptRuntime {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JavaScriptRuntimeDependency))
    }
}

/// Dependency on a Vala package.
///
/// This represents a dependency on a Vala package that can be located
/// using pkg-config.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValaPackageDependency {
    /// Name of the Vala package
    package: String,
}

impl ValaPackageDependency {
    /// Create a new ValaPackageDependency.
    ///
    /// # Arguments
    /// * `package` - Name of the Vala package
    ///
    /// # Returns
    /// A new ValaPackageDependency instance
    pub fn new(package: &str) -> Self {
        Self {
            package: package.to_string(),
        }
    }
}

impl Dependency for ValaPackageDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "vala-package"
    fn family(&self) -> &'static str {
        "vala-package"
    }

    /// Checks if the dependency is present in the project context.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Checks if the Vala package is available in the system.
    ///
    /// Uses pkg-config to check if the package exists.
    ///
    /// # Arguments
    /// * `session` - The session in which to check
    ///
    /// # Returns
    /// `true` if the package exists, `false` otherwise
    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["pkg-config", "--exists", &self.package])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for ValaPackageDependency {
    /// Convert this dependency to a list of Debian package dependencies.
    ///
    /// Attempts to find the Debian packages that provide the Vala package by
    /// searching for .vapi files in standard locations.
    ///
    /// # Arguments
    /// * `apt` - The APT package manager to use for queries
    ///
    /// # Returns
    /// A list of Debian package dependencies if found, or None if not found
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(
            apt.get_packages_for_paths(
                vec![&format!(
                    "/usr/share/vala-[.0-9]+/vapi/{}\\.vapi",
                    regex::escape(&self.package)
                )],
                true,
                false,
            )
            .unwrap()
            .iter()
            .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
            .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingValaPackage {
    /// Convert a MissingValaPackage problem to a Dependency.
    ///
    /// # Returns
    /// A ValaPackageDependency boxed as a Dependency trait object
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(ValaPackageDependency::new(&self.0)))
    }
}

/// Dependency on a Ruby gem.
///
/// This represents a dependency on a Ruby gem that can be installed
/// via RubyGems or bundler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubyGemDependency {
    /// Name of the Ruby gem
    gem: String,
    /// Optional minimum version requirement
    minimum_version: Option<String>,
}

impl RubyGemDependency {
    /// Create a new RubyGemDependency with optional version constraint.
    ///
    /// # Arguments
    /// * `gem` - Name of the Ruby gem
    /// * `minimum_version` - Optional minimum version requirement
    ///
    /// # Returns
    /// A new RubyGemDependency instance
    pub fn new(gem: &str, minimum_version: Option<&str>) -> Self {
        Self {
            gem: gem.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    /// Create a new RubyGemDependency without version constraints.
    ///
    /// # Arguments
    /// * `gem` - Name of the Ruby gem
    ///
    /// # Returns
    /// A new RubyGemDependency instance without version constraints
    pub fn simple(gem: &str) -> Self {
        Self {
            gem: gem.to_string(),
            minimum_version: None,
        }
    }
}

impl Dependency for RubyGemDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "ruby-gem"
    fn family(&self) -> &'static str {
        "ruby-gem"
    }

    /// Checks if the gem is present in the project's bundle.
    ///
    /// Uses the `bundle list` command to check if the gem is available
    /// with the required version in the project's Gemfile.
    ///
    /// # Arguments
    /// * `session` - The session in which to check
    ///
    /// # Returns
    /// `true` if the gem exists in the project's bundle, `false` otherwise
    fn project_present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["bundle".to_string(), "list".to_string()];
        if let Some(minimum_version) = &self.minimum_version {
            cmd.push(format!(">={}", minimum_version));
        }
        cmd.push(self.gem.clone());
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    /// Checks if the gem is installed in the system.
    ///
    /// Uses the `gem list --local` command to check if the gem is available
    /// with the required version.
    ///
    /// # Arguments
    /// * `session` - The session in which to check
    ///
    /// # Returns
    /// `true` if the gem exists in the system, `false` otherwise
    fn present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["gem".to_string(), "list".to_string(), "--local".to_string()];
        if let Some(minimum_version) = &self.minimum_version {
            cmd.push(format!(">={}", minimum_version));
        }
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for RubyGemDependency {
    /// Convert this dependency to a list of Debian package dependencies.
    ///
    /// Attempts to find the Debian packages that provide the Ruby gem by
    /// searching for .gemspec files in standard locations.
    ///
    /// # Arguments
    /// * `apt` - The APT package manager to use for queries
    ///
    /// # Returns
    /// A list of Debian package dependencies if found, or None if not found
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![
                    std::path::Path::new("/usr/share/rubygems-integration/all/specifications/")
                        .join(format!("{}-.*\\.gemspec", regex::escape(&self.gem)).as_str())
                        .to_str()
                        .unwrap(),
                ],
                true,
                false,
            )
            .unwrap();
        if names.is_empty() {
            return None;
        }
        Some(
            names
                .into_iter()
                .map(|name| {
                    if let Some(min_version) = self.minimum_version.as_ref() {
                        crate::dependencies::debian::DebianDependency::new_with_min_version(
                            &name,
                            &min_version.parse().unwrap(),
                        )
                    } else {
                        crate::dependencies::debian::DebianDependency::simple(&name)
                    }
                })
                .collect(),
        )
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::FromDebianDependency for RubyGemDependency {
    /// Create a RubyGemDependency from a Debian dependency.
    ///
    /// Extracts the gem name and version from a Debian package name,
    /// assuming the package name follows the ruby-* naming convention.
    ///
    /// # Arguments
    /// * `dependency` - The Debian dependency to convert
    ///
    /// # Returns
    /// A RubyGemDependency boxed as a Dependency trait object if conversion is possible,
    /// None otherwise
    fn from_debian_dependency(
        dependency: &crate::dependencies::debian::DebianDependency,
    ) -> Option<Box<dyn Dependency>> {
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;
        let (_, name) = lazy_regex::regex_captures!(r"ruby-(.*)", &name)?;

        Some(Box::new(Self {
            gem: name.to_string(),
            minimum_version: min_version.map(|v| v.upstream_version.to_string()),
        }))
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingRubyGem {
    /// Convert a MissingRubyGem problem to a Dependency.
    ///
    /// # Returns
    /// A RubyGemDependency boxed as a Dependency trait object
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RubyGemDependency::new(
            &self.gem,
            self.version.as_ref().map(|s| s.as_str()),
        )))
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for RubyGemDependency {
    /// Find upstream metadata for this Ruby gem.
    ///
    /// Uses the upstream-ontologist crate to fetch metadata about the gem
    /// from RubyGems.org.
    ///
    /// # Returns
    /// Upstream metadata if available, None otherwise
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(upstream_ontologist::providers::ruby::remote_rubygem_metadata(&self.gem))
            .ok()
    }
}

/// Dependency on a Debian debhelper addon.
///
/// This represents a dependency on a debhelper addon that can be used
/// in Debian packaging with the `dh` command.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhAddonDependency {
    /// Name of the debhelper addon
    addon: String,
}

impl DhAddonDependency {
    /// Create a new DhAddonDependency.
    ///
    /// # Arguments
    /// * `addon` - Name of the debhelper addon
    ///
    /// # Returns
    /// A new DhAddonDependency instance
    pub fn new(addon: &str) -> Self {
        Self {
            addon: addon.to_string(),
        }
    }
}

impl Dependency for DhAddonDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "dh-addon"
    fn family(&self) -> &'static str {
        "dh-addon"
    }

    /// Checks if the dependency is present in the system.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Checks if the dependency is present in the project context.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for DhAddonDependency {
    /// Convert this dependency to a list of Debian package dependencies.
    ///
    /// Attempts to find the Debian packages that provide the debhelper addon by
    /// searching for addon Perl modules in standard locations.
    ///
    /// # Arguments
    /// * `apt` - The APT package manager to use for queries
    ///
    /// # Returns
    /// A list of Debian package dependencies if found, or None if not found
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(
            apt.get_packages_for_paths(
                vec![&format!(
                    "/usr/share/perl5/Debian/Debhelper/Sequence/{}.pm",
                    regex::escape(&self.addon)
                )],
                true,
                false,
            )
            .unwrap()
            .iter()
            .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
            .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::DhAddonLoadFailure {
    /// Convert a DhAddonLoadFailure problem to a Dependency.
    ///
    /// # Returns
    /// A DhAddonDependency boxed as a Dependency trait object
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(DhAddonDependency::new(&self.path)))
    }
}

/// Dependency on a system library.
///
/// This represents a dependency on a shared library that can be linked
/// against using the -l flag with the linker.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDependency {
    /// Name of the library (without the lib prefix)
    library: String,
}

impl LibraryDependency {
    /// Create a new LibraryDependency.
    ///
    /// # Arguments
    /// * `library` - Name of the library (without the lib prefix)
    ///
    /// # Returns
    /// A new LibraryDependency instance
    pub fn new(library: &str) -> Self {
        Self {
            library: library.to_string(),
        }
    }
}

impl Dependency for LibraryDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "library"
    fn family(&self) -> &'static str {
        "library"
    }

    /// Checks if the library is present in the system.
    ///
    /// Uses the `ld` command to check if the library can be linked against.
    ///
    /// # Arguments
    /// * `session` - The session in which to check
    ///
    /// # Returns
    /// `true` if the library exists and can be linked against, `false` otherwise
    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["ld", "-l", &self.library])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    /// Checks if the dependency is present in the project context.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for LibraryDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = vec![
            format!("/usr/lib/lib{}.so", &self.library),
            format!("/usr/lib/.*/lib{}.so", regex::escape(&self.library)),
            format!("/usr/lib/lib{}.a", &self.library),
            format!("/usr/lib/.*/lib{}.a", regex::escape(&self.library)),
        ];
        Some(
            apt.get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingLibrary {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(LibraryDependency::new(&self.0)))
    }
}

/// Dependency on a static library.
///
/// This represents a dependency on a static library (.a file) that
/// can be linked against at build time.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticLibraryDependency {
    /// Name of the library (without the lib prefix)
    library: String,
    /// Filename of the library archive
    filename: String,
}

impl StaticLibraryDependency {
    /// Create a new StaticLibraryDependency.
    ///
    /// # Arguments
    /// * `library` - Name of the library (without the lib prefix)
    /// * `filename` - Filename of the library archive
    ///
    /// # Returns
    /// A new StaticLibraryDependency instance
    pub fn new(library: &str, filename: &str) -> Self {
        Self {
            library: library.to_string(),
            filename: filename.to_string(),
        }
    }
}

impl Dependency for StaticLibraryDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "static-library"
    fn family(&self) -> &'static str {
        "static-library"
    }

    /// Checks if the static library is present in the system.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Checks if the dependency is present in the project context.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for StaticLibraryDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = vec![
            format!("/usr/lib/lib{}.a", &self.library),
            format!("/usr/lib/.*/lib{}.a", regex::escape(&self.library)),
        ];
        Some(
            apt.get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingStaticLibrary {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(StaticLibraryDependency::new(
            &self.library,
            &self.filename,
        )))
    }
}

/// Dependency on a Ruby source file.
///
/// This represents a dependency on a Ruby source file that can be
/// loaded with the require() function in Ruby.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubyFileDependency {
    /// Name of the Ruby file (without .rb extension)
    filename: String,
}

impl RubyFileDependency {
    /// Create a new RubyFileDependency.
    ///
    /// # Arguments
    /// * `filename` - Name of the Ruby file (without .rb extension)
    ///
    /// # Returns
    /// A new RubyFileDependency instance
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
        }
    }
}

impl Dependency for RubyFileDependency {
    fn family(&self) -> &'static str {
        "ruby-file"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["ruby", "-e", &format!("require '{}'", self.filename)])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for RubyFileDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = vec![format!(
            "/usr/lib/ruby/vendor_ruby/{}.rb",
            regex::escape(&self.filename)
        )];
        let mut names = apt
            .get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), false, false)
            .unwrap();

        if names.is_empty() {
            let paths = vec![format!(
                "/usr/share/rubygems\\-integration/all/gems/([^/]+)/lib/{}\\.rb",
                regex::escape(&self.filename)
            )];
            names = apt
                .get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, false)
                .unwrap();
        }

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingRubyFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RubyFileDependency::new(&self.filename)))
    }
}

/// Dependency on a file managed by Sprockets asset pipeline.
///
/// This represents a dependency on a file that can be processed by the
/// Sprockets asset pipeline in Ruby on Rails.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprocketsFileDependency {
    /// MIME type of the file (e.g., "application/javascript")
    content_type: String,
    /// Name of the asset
    name: String,
}

impl SprocketsFileDependency {
    /// Create a new SprocketsFileDependency.
    ///
    /// # Arguments
    /// * `content_type` - MIME type of the file
    /// * `name` - Name of the asset
    ///
    /// # Returns
    /// A new SprocketsFileDependency instance
    pub fn new(content_type: &str, name: &str) -> Self {
        Self {
            content_type: content_type.to_string(),
            name: name.to_string(),
        }
    }
}

impl Dependency for SprocketsFileDependency {
    fn family(&self) -> &'static str {
        "sprockets-file"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["sprockets", "--check", &self.name])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for SprocketsFileDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let path = match self.content_type.as_str() {
            "application/javascript" => format!(
                "/usr/share/,*/app/assets/javascripts/{}\\.js",
                regex::escape(&self.name)
            ),
            _ => return None,
        };
        Some(
            apt.get_packages_for_paths(vec![&path], true, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingSprocketsFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(SprocketsFileDependency::new(
            &self.content_type,
            &self.name,
        )))
    }
}

/// Dependency on a CMake module or config file.
///
/// This represents a dependency on a CMake module or configuration file
/// that can be found with find_package() in CMake.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CMakeFileDependency {
    /// Name of the CMake file
    filename: String,
    /// Optional version requirement
    version: Option<String>,
}

impl CMakeFileDependency {
    /// Create a new CMakeFileDependency with optional version requirement.
    ///
    /// # Arguments
    /// * `filename` - Name of the CMake file
    /// * `version` - Optional version requirement
    ///
    /// # Returns
    /// A new CMakeFileDependency instance
    pub fn new(filename: &str, version: Option<&str>) -> Self {
        Self {
            filename: filename.to_string(),
            version: version.map(|s| s.to_string()),
        }
    }

    /// Create a new CMakeFileDependency without version requirement.
    ///
    /// # Arguments
    /// * `filename` - Name of the CMake file
    ///
    /// # Returns
    /// A new CMakeFileDependency instance without version requirement
    pub fn simple(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            version: None,
        }
    }
}

impl Dependency for CMakeFileDependency {
    fn family(&self) -> &'static str {
        "cmakefile"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for CMakeFileDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let paths = vec![
            format!("/usr/lib/.*/cmake/.*/{}", regex::escape(&self.filename)),
            format!("/usr/share/.*/cmake/{}", regex::escape(&self.filename)),
        ];
        Some(
            apt.get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::CMakeFilesMissing {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(CMakeFileDependency::new(
            &self.filenames[0],
            self.version.as_ref().map(|s| s.as_str()),
        )))
    }
}

/// Type of Maven artifact.
///
/// Represents different kinds of Maven artifacts that can be dependencies.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MavenArtifactKind {
    /// Java archive (JAR) file
    #[default]
    Jar,
    /// Project Object Model (POM) file
    Pom,
}

impl std::fmt::Display for MavenArtifactKind {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            MavenArtifactKind::Jar => write!(f, "jar"),
            MavenArtifactKind::Pom => write!(f, "pom"),
        }
    }
}

impl std::str::FromStr for MavenArtifactKind {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "jar" => Ok(MavenArtifactKind::Jar),
            "pom" => Ok(MavenArtifactKind::Pom),
            _ => Err("Invalid Maven artifact kind".to_string()),
        }
    }
}

/// Dependency on a Maven artifact.
///
/// This represents a dependency on a Maven artifact identified by
/// group ID, artifact ID, and optionally version and kind.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenArtifactDependency {
    /// Maven group ID (e.g., "org.apache.maven")
    pub group_id: String,
    /// Maven artifact ID (e.g., "maven-core")
    pub artifact_id: String,
    /// Optional version requirement (e.g., "3.8.6")
    pub version: Option<String>,
    /// Optional kind of artifact (JAR or POM)
    pub kind: Option<MavenArtifactKind>,
}

impl MavenArtifactDependency {
    /// Create a new MavenArtifactDependency with all details.
    ///
    /// # Arguments
    /// * `group_id` - Maven group ID
    /// * `artifact_id` - Maven artifact ID
    /// * `version` - Optional version requirement
    /// * `kind` - Optional artifact kind string ("jar" or "pom")
    ///
    /// # Returns
    /// A new MavenArtifactDependency instance
    pub fn new(
        group_id: &str,
        artifact_id: &str,
        version: Option<&str>,
        kind: Option<&str>,
    ) -> Self {
        Self {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
            version: version.map(|s| s.to_string()),
            kind: kind.map(|s| s.parse().unwrap()),
        }
    }

    /// Create a new MavenArtifactDependency with just group and artifact IDs.
    ///
    /// # Arguments
    /// * `group_id` - Maven group ID
    /// * `artifact_id` - Maven artifact ID
    ///
    /// # Returns
    /// A new MavenArtifactDependency instance without version or kind specified
    pub fn simple(group_id: &str, artifact_id: &str) -> Self {
        Self {
            group_id: group_id.to_string(),
            artifact_id: artifact_id.to_string(),
            version: None,
            kind: None,
        }
    }
}

impl From<(String, String)> for MavenArtifactDependency {
    fn from((group_id, artifact_id): (String, String)) -> Self {
        Self {
            group_id,
            artifact_id,
            version: None,
            kind: Some(MavenArtifactKind::Jar),
        }
    }
}

impl From<(String, String, String)> for MavenArtifactDependency {
    fn from((group_id, artifact_id, version): (String, String, String)) -> Self {
        Self {
            group_id,
            artifact_id,
            version: Some(version),
            kind: Some(MavenArtifactKind::Jar),
        }
    }
}

impl From<(String, String, String, String)> for MavenArtifactDependency {
    fn from((group_id, artifact_id, version, kind): (String, String, String, String)) -> Self {
        Self {
            group_id,
            artifact_id,
            version: Some(version),
            kind: Some(kind.parse().unwrap()),
        }
    }
}

impl Dependency for MavenArtifactDependency {
    fn family(&self) -> &'static str {
        "maven-artifact"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for MavenArtifactDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let group_id = self.group_id.replace(".", "/");
        let kind = self.kind.clone().unwrap_or_default().to_string();
        let (path, regex) = if let Some(version) = self.version.as_ref() {
            (
                std::path::Path::new("/usr/share/maven-repo")
                    .join(group_id)
                    .join(&self.artifact_id)
                    .join(version)
                    .join(format!("{}-{}.{}", self.artifact_id, version, kind)),
                true,
            )
        } else {
            (
                std::path::Path::new("/usr/share/maven-repo")
                    .join(regex::escape(&group_id))
                    .join(regex::escape(&self.artifact_id))
                    .join(".*")
                    .join(format!(
                        "{}-.*\\.{}",
                        regex::escape(&self.artifact_id),
                        kind
                    )),
                false,
            )
        };

        let names = apt
            .get_packages_for_paths(vec![path.to_str().unwrap()], regex, false)
            .unwrap();
        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl std::str::FromStr for MavenArtifactDependency {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let parts: Vec<&str> = s.split(':').collect();
        match parts.len() {
            2 => Ok(Self::from((parts[0].to_string(), parts[1].to_string()))),
            3 => Ok(Self::from((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
            ))),
            4 => Ok(Self::from((
                parts[0].to_string(),
                parts[1].to_string(),
                parts[2].to_string(),
                parts[3].to_string(),
            ))),
            _ => Err("Invalid Maven artifact dependency".to_string()),
        }
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingMavenArtifacts {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        let text = self.0[0].as_str();
        let d: MavenArtifactDependency = text.parse().unwrap();
        Some(Box::new(d))
    }
}

/// Dependency on GNOME common build tools.
///
/// This represents a dependency on the gnome-common package which provides
/// common build infrastructure for GNOME projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnomeCommonDependency;

impl Dependency for GnomeCommonDependency {
    fn family(&self) -> &'static str {
        "gnome-common"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["gnome-autogen.sh", "--version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for GnomeCommonDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new(
            "gnome-common",
        )])
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingGnomeCommonDependency {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(GnomeCommonDependency))
    }
}

/// Dependency on a Qt module.
///
/// This represents a dependency on a Qt module like QtCore, QtGui, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QtModuleDependency {
    /// Name of the Qt module
    module: String,
}

impl QtModuleDependency {
    /// Create a new QtModuleDependency.
    ///
    /// # Arguments
    /// * `module` - Name of the Qt module
    ///
    /// # Returns
    /// A new QtModuleDependency instance
    pub fn new(module: &str) -> Self {
        Self {
            module: module.to_string(),
        }
    }
}

impl Dependency for QtModuleDependency {
    fn family(&self) -> &'static str {
        "qt-module"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for QtModuleDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![&format!(
                    "/usr/lib/.*/qt5/mkspecs/modules/qt_lib_{}\\.pri",
                    regex::escape(&self.module)
                )],
                true,
                false,
            )
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingQtModules {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(QtModuleDependency::new(&self.0[0])))
    }
}

/// Dependency on the Qt toolkit.
///
/// This represents a dependency on the Qt toolkit as a whole,
/// specifically the qmake tool for building Qt projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QTDependency;

impl Dependency for QTDependency {
    fn family(&self) -> &'static str {
        "qt"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for QTDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(vec!["/usr/lib/.*/qt[0-9]+/bin/qmake"], true, false)
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingQt {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(QTDependency))
    }
}

/// Dependency on the X11 window system.
///
/// This represents a dependency on the X Window System (X11),
/// which is needed for graphical applications.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct X11Dependency;

impl Dependency for X11Dependency {
    fn family(&self) -> &'static str {
        "x11"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // Does the X binary exist?
        crate::session::which(session, "X").is_some()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for X11Dependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new(
            "libx11-dev",
        )])
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingX11 {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(X11Dependency))
    }
}

/// Dependency on certificate authority certificates.
///
/// This represents a dependency on CA certificates needed for
/// secure HTTPS connections to specific URLs.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateAuthorityDependency {
    /// URL that requires certificate verification
    url: String,
}

impl CertificateAuthorityDependency {
    /// Create a new CertificateAuthorityDependency.
    ///
    /// # Arguments
    /// * `url` - URL that requires certificate verification
    ///
    /// # Returns
    /// A new CertificateAuthorityDependency instance
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

impl Dependency for CertificateAuthorityDependency {
    fn family(&self) -> &'static str {
        "certificate-authority"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for CertificateAuthorityDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::simple(
            "ca-certificates",
        )])
    }
}

impl ToDependency for buildlog_consultant::problems::common::UnknownCertificateAuthority {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(CertificateAuthorityDependency::new(&self.0)))
    }
}

/// Dependency on the GNU Libtool.
///
/// This represents a dependency on the GNU Libtool, which is used to
/// create portable libraries in autotools-based projects.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibtoolDependency;

impl Dependency for LibtoolDependency {
    fn family(&self) -> &'static str {
        "libtool"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["libtoolize", "--version"])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for LibtoolDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new(
            "libtool",
        )])
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingLibtool {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(LibtoolDependency))
    }
}

/// Dependency on a Boost library component.
///
/// This represents a dependency on a specific component of the Boost C++ Libraries,
/// such as boost_system, boost_filesystem, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostComponentDependency {
    /// Name of the Boost component
    name: String,
}

impl BoostComponentDependency {
    /// Create a new BoostComponentDependency.
    ///
    /// # Arguments
    /// * `name` - Name of the Boost component
    ///
    /// # Returns
    /// A new BoostComponentDependency instance
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Dependency for BoostComponentDependency {
    fn family(&self) -> &'static str {
        "boost-component"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for BoostComponentDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![&format!(
                    "/usr/lib/.*/libboost_{}",
                    regex::escape(&self.name)
                )],
                true,
                false,
            )
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

/// Dependency on a KDE Frameworks 5 component.
///
/// This represents a dependency on a specific component of the
/// KDE Frameworks 5, such as KF5Auth, KF5CoreAddons, etc.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KF5ComponentDependency {
    /// Name of the KF5 component
    name: String,
}

impl KF5ComponentDependency {
    /// Create a new KF5ComponentDependency.
    ///
    /// # Arguments
    /// * `name` - Name of the KF5 component
    ///
    /// # Returns
    /// A new KF5ComponentDependency instance
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Dependency for KF5ComponentDependency {
    fn family(&self) -> &'static str {
        "kf5-component"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for KF5ComponentDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![&format!(
                    "/usr/lib/.*/cmake/KF5{}/KF5{}Config\\.cmake",
                    regex::escape(&self.name),
                    regex::escape(&self.name)
                )],
                true,
                false,
            )
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCMakeComponents {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        match self.name.as_str() {
            "Boost" => Some(Box::new(BoostComponentDependency::new(&self.components[0]))),
            "KF5" => Some(Box::new(KF5ComponentDependency::new(&self.components[0]))),
            n => {
                log::warn!("Unknown CMake component: {}", n);
                None
            }
        }
    }
}

/// Dependency on a Gnulib directory.
///
/// This represents a dependency on a specific directory containing
/// Gnulib code, which is a collection of portable C functions.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnulibDirectoryDependency {
    /// Path to the Gnulib directory
    directory: PathBuf,
}

impl GnulibDirectoryDependency {
    /// Create a new GnulibDirectoryDependency.
    ///
    /// # Arguments
    /// * `directory` - Path to the Gnulib directory
    ///
    /// # Returns
    /// A new GnulibDirectoryDependency instance
    pub fn new(directory: &str) -> Self {
        Self {
            directory: PathBuf::from(directory),
        }
    }
}

impl Dependency for GnulibDirectoryDependency {
    fn family(&self) -> &'static str {
        "gnulib-directory"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingGnulibDirectory {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(GnulibDirectoryDependency {
            directory: PathBuf::from(self.0.clone()),
        }))
    }
}

/// Dependency on a GObject Introspection typelib.
///
/// This represents a dependency on a GObject Introspection typelib file,
/// which provides language bindings for GObject-based libraries.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionTypelibDependency {
    /// Name of the library with introspection data
    library: String,
}

impl IntrospectionTypelibDependency {
    /// Create a new IntrospectionTypelibDependency.
    ///
    /// # Arguments
    /// * `library` - Name of the library with introspection data
    ///
    /// # Returns
    /// A new IntrospectionTypelibDependency instance
    pub fn new(library: &str) -> Self {
        Self {
            library: library.to_string(),
        }
    }
}

impl Dependency for IntrospectionTypelibDependency {
    fn family(&self) -> &'static str {
        "introspection-type-lib"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingIntrospectionTypelib {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(IntrospectionTypelibDependency::new(&self.0)))
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for IntrospectionTypelibDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<crate::dependencies::debian::DebianDependency>> {
        let names = apt
            .get_packages_for_paths(
                vec![&format!(
                    "/usr/lib/.*/girepository\\-.*/{}\\-.*.typelib",
                    regex::escape(&self.library)
                )],
                true,
                false,
            )
            .unwrap();

        if names.is_empty() {
            return None;
        }

        Some(
            names
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingCSharpCompiler {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(BinaryDependency::new("mcs")))
    }
}

impl ToDependency for buildlog_consultant::problems::common::MissingXfceDependency {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        match self.package.as_str() {
            "gtk-doc" => Some(Box::new(BinaryDependency::new("gtkdocize"))),
            n => {
                log::warn!("Unknown XFCE dependency: {}", n);
                None
            }
        }
    }
}
