//! Support for Rust build systems.
//!
//! This module provides functionality for building, testing, and managing
//! dependencies for Rust projects using Cargo.

use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::CargoCrateDependency;
use crate::dependency::Dependency;
use std::path::{Path, PathBuf};

/// A Cargo package declaration from Cargo.toml.
#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct Package {
    name: String,
}

/// A dependency declaration in a Cargo.toml file.
///
/// This can be either a simple version string or a detailed declaration
/// with additional configuration options.
#[derive(serde::Deserialize, Debug)]
#[serde(untagged)]
#[allow(dead_code)]
enum CrateDependency {
    /// Simple version string dependency
    Version(String),
    /// Detailed dependency with configuration options
    Details {
        /// Version requirement string
        version: Option<String>,
        /// Whether the dependency is optional
        optional: Option<bool>,
        /// List of features to enable
        features: Option<Vec<String>>,
        /// Whether to use the workspace version
        workspace: Option<bool>,
        /// Git repository URL
        git: Option<String>,
        /// Git branch to use
        branch: Option<String>,
        /// Whether to enable default features
        #[serde(rename = "default-features")]
        default_features: Option<bool>,
    },
}

/// Methods for accessing CrateDependency information.
#[allow(dead_code)]
impl CrateDependency {
    /// Get the version string if available.
    ///
    /// # Returns
    /// The version string, or None if not specified
    fn version(&self) -> Option<&str> {
        match self {
            Self::Version(v) => Some(v.as_str()),
            Self::Details { version, .. } => version.as_deref(),
        }
    }

    /// Get the list of features if available.
    ///
    /// # Returns
    /// A slice of feature strings, or None if not specified
    fn features(&self) -> Option<&[String]> {
        match self {
            Self::Version(_) => None,
            Self::Details { features, .. } => features.as_ref().map(|v| v.as_slice()),
        }
    }
}

/// A binary target declared in a Cargo.toml file.
#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
pub struct CrateBinary {
    /// Name of the binary
    name: String,
    /// Path to the binary source file
    path: Option<PathBuf>,
    /// List of features that must be enabled for this binary to be built
    #[serde(rename = "required-features")]
    required_features: Option<Vec<String>>,
}

/// A library target declared in a Cargo.toml file.
#[derive(serde::Deserialize, Debug)]
pub struct CrateLibrary {}

/// Representation of a Cargo.toml file.
#[derive(serde::Deserialize, Debug)]
#[allow(dead_code)]
struct CargoToml {
    /// Package metadata
    package: Option<Package>,
    /// Map of dependency name to dependency details
    dependencies: Option<std::collections::HashMap<String, CrateDependency>>,
    /// List of binary targets
    bin: Option<Vec<CrateBinary>>,
    /// Library target (if any)
    lib: Option<CrateLibrary>,
}

/// Cargo build system for Rust projects.
///
/// This build system handles Rust projects that use Cargo for building,
/// testing, and dependency management.
#[derive(Debug)]
pub struct Cargo {
    /// Path to the Cargo project
    #[allow(dead_code)]
    path: PathBuf,
    /// Parsed Cargo.toml file
    local_crate: CargoToml,
}

impl Cargo {
    /// Create a new Cargo build system.
    ///
    /// # Arguments
    /// * `path` - Path to the Cargo project
    ///
    /// # Returns
    /// A new Cargo instance with parsed Cargo.toml
    pub fn new(path: PathBuf) -> Self {
        let cargo_toml = std::fs::read_to_string(path.join("Cargo.toml")).unwrap();
        let local_crate: CargoToml = toml::from_str(&cargo_toml).unwrap();
        Self { path, local_crate }
    }

    /// Probe a directory to check if it contains a Cargo project.
    ///
    /// # Arguments
    /// * `path` - Path to check for a Cargo.toml file
    ///
    /// # Returns
    /// Some(BuildSystem) if a Cargo.toml is found, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("Cargo.toml").exists() {
            log::debug!("Found Cargo.toml, assuming rust cargo package.");
            Some(Box::new(Cargo::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

/// Implementation of BuildSystem for Cargo.
impl BuildSystem for Cargo {
    /// Get the name of this build system.
    ///
    /// # Returns
    /// The string "cargo"
    fn name(&self) -> &str {
        "cargo"
    }

    /// Create a distribution package.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    /// * `_target_directory` - Directory to store the created distribution package
    /// * `_quiet` - Whether to suppress output
    ///
    /// # Returns
    /// Always returns Error::Unimplemented as dist is not implemented for Cargo
    fn dist(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &std::path::Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        Err(Error::Unimplemented)
    }

    /// Run tests using cargo test command.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn test(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        session
            .command(vec!["cargo", "test"])
            .run_detecting_problems()?;
        Ok(())
    }

    /// Build the project using cargo build command.
    ///
    /// Attempts to run cargo generate first, if available.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn build(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        match session
            .command(vec!["cargo", "generate"])
            .run_detecting_problems()
        {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines == ["error: no such subcommand: `generate`"] => {}
            Err(e) => return Err(e.into()),
        }
        session
            .command(vec!["cargo", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    /// Clean build artifacts using cargo clean command.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn clean(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        session
            .command(vec!["cargo", "clean"])
            .run_detecting_problems()?;
        Ok(())
    }

    /// Install the built software using cargo install command.
    ///
    /// # Arguments
    /// * `session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    /// * `install_target` - Target installation directory
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn install(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        let mut args = vec![
            "cargo".to_string(),
            "install".to_string(),
            "--path=.".to_string(),
        ];
        if let Some(prefix) = install_target.prefix.as_ref() {
            args.push(format!("-root={}", prefix.to_str().unwrap()));
        }
        session
            .command(args.iter().map(|x| x.as_str()).collect())
            .run_detecting_problems()?;
        Ok(())
    }

    /// Get dependencies declared in the Cargo.toml file.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// A list of dependencies with their categories
    fn get_declared_dependencies(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(
            crate::buildsystem::DependencyCategory,
            Box<dyn crate::dependency::Dependency>,
        )>,
        Error,
    > {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        for (name, details) in self
            .local_crate
            .dependencies
            .as_ref()
            .unwrap_or(&std::collections::HashMap::new())
        {
            ret.push((
                DependencyCategory::Build,
                Box::new(CargoCrateDependency {
                    name: name.clone(),
                    features: Some(details.features().unwrap_or(&[]).to_vec()),
                    api_version: None,
                    minimum_version: None,
                }),
            ));
        }
        Ok(ret)
    }

    /// Get outputs declared in the Cargo.toml file.
    ///
    /// # Arguments
    /// * `_session` - Session to run commands in
    /// * `_fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// A list of binary outputs from the bin section of Cargo.toml
    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        let mut ret: Vec<Box<dyn crate::output::Output>> = vec![];
        if let Some(bins) = &self.local_crate.bin {
            for bin in bins {
                ret.push(Box::new(crate::output::BinaryOutput::new(&bin.name)));
            }
        }
        // TODO: library output
        Ok(ret)
    }

    /// Install declared dependencies using cargo fetch.
    ///
    /// # Arguments
    /// * `_categories` - Categories of dependencies to install
    /// * `scopes` - Installation scopes to consider
    /// * `session` - Session to run commands in
    /// * `_installer` - Installer to use for installing dependencies
    /// * `fixers` - Build fixers to use if needed
    ///
    /// # Returns
    /// Ok on success, Error otherwise
    fn install_declared_dependencies(
        &self,
        _categories: &[crate::buildsystem::DependencyCategory],
        scopes: &[crate::installer::InstallationScope],
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<(), Error> {
        if !scopes.contains(&crate::installer::InstallationScope::Vendor) {
            return Err(crate::installer::Error::UnsupportedScopes(scopes.to_vec()).into());
        }
        if let Some(fixers) = fixers {
            session
                .command(vec!["cargo", "fetch"])
                .run_fixing_problems::<_, Error>(fixers)
                .unwrap();
        } else {
            session
                .command(vec!["cargo", "fetch"])
                .run_detecting_problems()?;
        }
        Ok(())
    }

    /// Convert this build system to Any for downcasting.
    ///
    /// # Returns
    /// Reference to self as Any
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
