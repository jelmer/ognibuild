use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::node::NodePackageDependency;
use crate::dependencies::BinaryDependency;
use crate::dependency::Dependency;
use crate::installer::{Error as InstallerError, InstallationScope, Installer};
use crate::session::Session;
use serde::Deserialize;
use std::collections::HashMap;
use std::path::PathBuf;

#[derive(Debug)]
#[allow(dead_code)]
/// Node.js build system.
///
/// Handles Node.js projects with a package.json file.
pub struct Node {
    path: PathBuf,
    package: NodePackage,
}

#[derive(Debug, Deserialize)]
struct NodePackage {
    dependencies: HashMap<String, String>,
    #[serde(rename = "devDependencies")]
    dev_dependencies: HashMap<String, String>,
    scripts: HashMap<String, String>,
}

impl Node {
    /// Create a new Node build system with the specified path to package.json.
    pub fn new(path: PathBuf) -> Result<Self, Box<dyn std::error::Error>> {
        let package_path = path.join("package.json");

        let package_content = std::fs::read_to_string(&package_path)?;

        let package: NodePackage = serde_json::from_str(&package_content)?;

        Ok(Self { path, package })
    }

    fn setup(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let binary_req = BinaryDependency::new("npm");
        if !binary_req.present(session) {
            installer.install(&binary_req, InstallationScope::Global)?;
        }
        Ok(())
    }

    /// Probe a directory for a Node.js build system.
    ///
    /// Returns a Node build system if a package.json file is found.
    pub fn probe(path: &std::path::Path) -> Option<Box<dyn BuildSystem>> {
        let package_json_path = path.join("package.json");
        if package_json_path.exists() {
            log::debug!("Found package.json, attempting to parse as node package.");
            match Self::new(path.to_path_buf()) {
                Ok(node_system) => return Some(Box::new(node_system)),
                Err(e) => {
                    log::debug!("Failed to parse package.json: {}", e);
                    return None;
                }
            }
        }
        None
    }
}

impl BuildSystem for Node {
    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        let mut dependencies: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];

        for (name, _version) in self.package.dependencies.iter() {
            // TODO(jelmer): Look at version
            dependencies.push((
                DependencyCategory::Universal,
                Box::new(NodePackageDependency::new(name)),
            ));
        }

        for (name, _version) in self.package.dev_dependencies.iter() {
            // TODO(jelmer): Look at version
            dependencies.push((
                DependencyCategory::Build,
                Box::new(NodePackageDependency::new(name)),
            ));
        }

        Ok(dependencies)
    }

    fn name(&self) -> &str {
        "node"
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        self.setup(session, installer)?;
        let dc = crate::dist_catcher::DistCatcher::new(vec![
            session.external_path(std::path::Path::new("."))
        ]);
        session
            .command(vec!["npm", "pack"])
            .quiet(quiet)
            .run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        if let Some(test_script) = self.package.scripts.get("test") {
            session
                .command(vec!["bash", "-c", test_script])
                .run_detecting_problems()?;
        } else {
            log::info!("No test command defined in package.json");
        }
        Ok(())
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        if let Some(build_script) = self.package.scripts.get("build") {
            session
                .command(vec!["bash", "-c", build_script])
                .run_detecting_problems()?;
        } else {
            log::info!("No build command defined in package.json");
        }
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        if let Some(clean_script) = self.package.scripts.get("clean") {
            session
                .command(vec!["bash", "-c", clean_script])
                .run_detecting_problems()?;
        } else {
            log::info!("No clean command defined in package.json");
        }
        Ok(())
    }

    fn install(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
