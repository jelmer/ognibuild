use crate::dependency::{Error, Dependency, Installer, Explanation};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[cfg(feature = "debian")]
pub mod debian;
pub mod go;
pub mod haskell;
pub mod java;
pub mod latex;
pub mod node;
pub mod octave;
pub mod perl;
pub mod php;
pub mod python;
pub mod r;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BinaryDependency {
    binary_name: String,
}

impl BinaryDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PytestPlugin {
    pub plugin: String,
}

impl PytestPlugin {
    pub fn new(plugin: &str) -> Self {
        Self {
            plugin: plugin.to_string(),
        }
    }
}

fn pytest_plugins(session: &dyn Session) -> Option<Vec<(String, String)>> {
    let output = session
        .command(vec!["pytest", "--version"])
        .stdout(std::process::Stdio::piped())
        .stderr(std::process::Stdio::null())
        .output()
        .unwrap();
    for line in String::from_utf8(output.stdout).unwrap().lines() {
        if let Some(rest) = line.strip_prefix("plugins: ") {
            return Some(
                rest.split(',')
                    .map(|s| {
                        let mut parts = s.splitn(2, '=');
                        (
                            parts.next().unwrap().to_string(),
                            parts.next().unwrap_or("").to_string(),
                        )
                    })
                    .collect(),
            );
        }
    }
    None
}

impl Dependency for PytestPlugin {
    fn family(&self) -> &'static str {
        "pytest-plugin"
    }

    fn present(&self, session: &dyn Session) -> bool {
        if let Some(plugins) = pytest_plugins(session) {
            plugins.iter().any(|(name, _)| name == &self.plugin)
        } else {
            false
        }
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VcsControlDirectoryAccessDependency {
    pub vcs: Vec<String>,
}

impl VcsControlDirectoryAccessDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct VagueDependency {
    pub name: String,
    pub minimum_version: Option<String>,
}

impl VagueDependency {
    pub fn new(name: &str, minimum_version: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(name: &str) -> Self {
        Self {
            name: name.to_string(),
            minimum_version: None,
        }
    }

    pub fn expand(&self) -> Vec<Box<dyn Dependency>> {
        let mut ret: Vec<Box<dyn Dependency>> = vec![];
        if !self.name.contains(' ') {
            ret.push(Box::new(BinaryDependency::new(&self.name)) as Box<dyn Dependency>);
            ret.push(Box::new(BinaryDependency::new(&self.name)) as Box<dyn Dependency>);
            ret.push(Box::new(PkgConfigDependency::new(
                &self.name.clone(),
                self.minimum_version.clone().as_deref(),
            )) as Box<dyn Dependency>);
            if self.name.to_lowercase() != self.name {
                ret.push(Box::new(BinaryDependency::new(&self.name.to_lowercase()))
                    as Box<dyn Dependency>);
                ret.push(Box::new(BinaryDependency::new(&self.name.to_lowercase()))
                    as Box<dyn Dependency>);
                ret.push(Box::new(PkgConfigDependency::new(
                    &self.name.to_lowercase(),
                    self.minimum_version.clone().as_deref(),
                )) as Box<dyn Dependency>);
            }
            #[cfg(feature = "apt")]
            {
                ret.push(Box::new(AptDependency::with_min_version(
                    self.name.to_lower(),
                    self.minimum_version.clone(),
                )));
                let devname = if self.name.to_lower().starts_with("lib") {
                    format!("{}-dev", self.name.to_lower())
                } else {
                    format!("lib{}-dev", self.name.to_lower())
                };
                ret.push(Box::new(AptDependency::with_min_version(
                    &devname,
                    self.minimum_version.clone(),
                )));
            }
        }
        ret
    }
}

impl Dependency for VagueDependency {
    fn family(&self) -> &'static str {
        "vague"
    }

    fn present(&self, session: &dyn Session) -> bool {
        self.expand().iter().any(|d| d.present(session))
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        self.expand().iter().any(|d| d.project_present(session))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LuaModuleDependency {
    module: String,
}

impl LuaModuleDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoCrateDependency {
    name: String,
    features: Option<Vec<String>>,
    api_version: Option<String>,
}

impl CargoCrateDependency {
    pub fn new(name: &str, features: Option<Vec<&str>>, api_version: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            features: features.map(|v| v.iter().map(|s| s.to_string()).collect()),
            api_version: api_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(name: &str) -> Self {
        Self {
            name: name.to_string(),
            features: None,
            api_version: None,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PkgConfigDependency {
    module: String,
    minimum_version: Option<String>,
}

impl PkgConfigDependency {
    pub fn new(module: &str, minimum_version: Option<&str>) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

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
        let mut cmd = vec![
            "pkg-config".to_string(),
            "--exists".to_string(),
            self.module.clone(),
        ];
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

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDependency {
    path: PathBuf,
}

impl PathDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CHeaderDependency {
    header: String,
}

impl CHeaderDependency {
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

    fn project_present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct JavaScriptRuntimeDependency;

impl Dependency for JavaScriptRuntimeDependency {
    fn family(&self) -> &'static str {
        "javascript-runtime"
    }

    fn project_present(&self, session: &dyn Session) -> bool {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ValaPackageDependency {
    package: String,
}

impl ValaPackageDependency {
    pub fn new(package: &str) -> Self {
        Self {
            package: package.to_string(),
        }
    }
}

impl Dependency for ValaPackageDependency {
    fn family(&self) -> &'static str {
        "vala-package"
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["pkg-config", "--exists", &self.package])
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubyGemDependency {
    gem: String,
    minimum_version: Option<String>,
}

impl RubyGemDependency {
    pub fn new(gem: &str, minimum_version: Option<&str>) -> Self {
        Self {
            gem: gem.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(gem: &str) -> Self {
        Self {
            gem: gem.to_string(),
            minimum_version: None,
        }
    }
}

impl Dependency for RubyGemDependency {
    fn family(&self) -> &'static str {
        "ruby-gem"
    }

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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DhAddonDependency {
    addon: String,
}

impl DhAddonDependency {
    pub fn new(addon: &str) -> Self {
        Self {
            addon: addon.to_string(),
        }
    }
}

impl Dependency for DhAddonDependency {
    fn family(&self) -> &'static str {
        "dh-addon"
    }

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct OctavePackageDependency {
    package: String,
    minimum_version: Option<String>,
}

impl OctavePackageDependency {
    pub fn new(package: &str, minimum_version: Option<&str>) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            minimum_version: None,
        }
    }
}

impl Dependency for OctavePackageDependency {
    fn family(&self) -> &'static str {
        "octave-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec![
                "octave",
                "--eval",
                &format!("pkg load {}", self.package),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct LibraryDependency {
    library: String,
}

impl LibraryDependency {
    pub fn new(library: &str) -> Self {
        Self {
            library: library.to_string(),
        }
    }
}

impl Dependency for LibraryDependency {
    fn family(&self) -> &'static str {
        "library"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["ld", "-l", &self.library])
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct StaticLibraryDependency {
    library: String,
    filename: String,
}

impl StaticLibraryDependency {
    pub fn new(library: &str, filename: &str) -> Self {
        Self {
            library: library.to_string(),
            filename: filename.to_string(),
        }
    }
}

impl Dependency for StaticLibraryDependency {
    fn family(&self) -> &'static str {
        "static-library"
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RubyFileDependency {
    filename: String,
}

impl RubyFileDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlEntityDependency {
    url: String,
}

impl XmlEntityDependency {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

impl Dependency for XmlEntityDependency {
    fn family(&self) -> &'static str {
        "xml-entity"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // Check if the entity is defined in the local XML catalog
        session
            .command(vec!["xmlcatalog", "--noout", "--resolve", &self.url])
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprocketsFile {
    content_type: String,
    name: String,
}

impl SprocketsFile {
    pub fn new(content_type: &str, name: &str) -> Self {
        Self {
            content_type: content_type.to_string(),
            name: name.to_string(),
        }
    }
}

impl Dependency for SprocketsFile {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CMakefileDependency {
    filename: String,
    version: Option<String>,
}

impl CMakefileDependency {
    pub fn new(filename: &str, version: Option<&str>) -> Self {
        Self {
            filename: filename.to_string(),
            version: version.map(|s| s.to_string()),
        }
    }

    pub fn simple(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
            version: None,
        }
    }
}

impl Dependency for CMakefileDependency {
    fn family(&self) -> &'static str {
        "cmakefile"
    }

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenArtifactDependency {
    group_id: String,
    artifact_id: String,
    version: Option<String>,
    kind: Option<String>,
}

impl MavenArtifactDependency {
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
            kind: kind.map(|s| s.to_string()),
        }
    }

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
            kind: Some("jar".to_string()),
        }
    }
}

impl From<(String, String, String)> for MavenArtifactDependency {
    fn from((group_id, artifact_id, version): (String, String, String)) -> Self {
        Self {
            group_id,
            artifact_id,
            version: Some(version),
            kind: Some("jar".to_string()),
        }
    }
}

impl From<(String, String, String, String)> for MavenArtifactDependency {
    fn from((group_id, artifact_id, version, kind): (String, String, String, String)) -> Self {
        Self {
            group_id,
            artifact_id,
            version: Some(version),
            kind: Some(kind),
        }
    }
}

impl Dependency for MavenArtifactDependency {
    fn family(&self) -> &'static str {
        "maven-artifact"
    }

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct QtModuleDependency {
    module: String,
}

impl QtModuleDependency {
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

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CertificateAuthorityDependency {
    url: String,
}

impl CertificateAuthorityDependency {
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

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoconfMacroDependency {
    macro_name: String,
}

impl AutoconfMacroDependency {
    pub fn new(macro_name: &str) -> Self {
        Self {
            macro_name: macro_name.to_string(),
        }
    }
}

impl Dependency for AutoconfMacroDependency {
    fn family(&self) -> &'static str {
        "autoconf-macro"
    }

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BoostComponentDependency {
    name: String,
}

impl BoostComponentDependency {
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

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct KF5ComponentDependency {
    name: String,
}

impl KF5ComponentDependency {
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

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnulibDirectoryDependency {
    directory: String,
}

impl GnulibDirectoryDependency {
    pub fn new(directory: &str) -> Self {
        Self {
            directory: directory.to_string(),
        }
    }
}

impl Dependency for GnulibDirectoryDependency {
    fn family(&self) -> &'static str {
        "gnulib-directory"
    }

    fn present(&self, session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct StackedInstaller(pub Vec<Box<dyn Installer>>);

impl StackedInstaller {
    pub fn new(resolvers: Vec<Box<dyn Installer>>) -> Self {
        Self(resolvers)
    }
}

impl Installer for StackedInstaller {
    fn install(&self, requirement: &dyn Dependency) -> Result<(), Error> {
        for sub in &self.0 {
            match sub.install(requirement) {
                Ok(()) => { return Ok(()); },
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => { return Err(e); }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }

    fn explain(&self, requirements: &dyn Dependency) -> Result<Explanation, Error> {
        for sub in &self.0 {
            match sub.explain(requirements) {
                Ok(e) => { return Ok(e); },
                Err(Error::UnknownDependencyFamily) => {}
                Err(e) => { return Err(e); }
            }
        }

        Err(Error::UnknownDependencyFamily)
    }
}
