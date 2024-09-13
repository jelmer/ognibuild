use crate::buildlog::ToDependency;
use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::collections::HashSet;
use std::path::PathBuf;

pub mod autoconf;
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
pub mod pytest;
pub mod python;
pub mod r;
pub mod vague;
pub mod xml;

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

impl ToDependency for buildlog_consultant::problems::common::MissingLuaModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(LuaModuleDependency::new(&self.0)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CargoCrateDependency {
    pub name: String,
    pub features: Option<Vec<String>>,
    pub api_version: Option<String>,
    pub minimum_version: Option<String>,
}

impl CargoCrateDependency {
    pub fn new(name: &str, features: Option<Vec<&str>>, api_version: Option<&str>) -> Self {
        Self {
            name: name.to_string(),
            features: features.map(|v| v.iter().map(|s| s.to_string()).collect()),
            api_version: api_version.map(|s| s.to_string()),
            minimum_version: None,
        }
    }

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
        upstream_ontologist::providers::rust::remote_crate_data(&self.name).ok()
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PathDependency {
    path: PathBuf,
}

impl From<PathBuf> for PathDependency {
    fn from(path: PathBuf) -> Self {
        Self { path }
    }
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

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for ValaPackageDependency {
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
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(ValaPackageDependency::new(&self.0)))
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

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for RubyGemDependency {
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
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(RubyGemDependency::new(
            &self.gem,
            self.version.as_ref().map(|s| s.as_str()),
        )))
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for RubyGemDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        upstream_ontologist::providers::ruby::remote_rubygem_metadata(&self.gem).ok()
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
impl crate::dependencies::debian::IntoDebianDependency for DhAddonDependency {
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
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(DhAddonDependency::new(&self.path)))
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SprocketsFileDependency {
    content_type: String,
    name: String,
}

impl SprocketsFileDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CMakeFileDependency {
    filename: String,
    version: Option<String>,
}

impl CMakeFileDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, Default)]
pub enum MavenArtifactKind {
    #[default]
    Jar,
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MavenArtifactDependency {
    pub group_id: String,
    pub artifact_id: String,
    pub version: Option<String>,
    pub kind: Option<MavenArtifactKind>,
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
            kind: kind.map(|s| s.parse().unwrap()),
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GnulibDirectoryDependency {
    directory: PathBuf,
}

impl GnulibDirectoryDependency {
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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct IntrospectionTypelibDependency {
    library: String,
}

impl IntrospectionTypelibDependency {
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
