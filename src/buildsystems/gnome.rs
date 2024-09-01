use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::vague::VagueDependency;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct GnomeShellExtension {
    path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct Metadata {
    name: String,
    description: String,
    uuid: String,
    shell_version: String,
    version: String,
    url: String,
    license: String,
    authors: Vec<String>,
    settings_schema: Option<String>,
    gettext_domain: Option<String>,
    extension: Option<String>,
    _generated: Option<bool>,
}

impl GnomeShellExtension {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn exists(path: &PathBuf) -> bool {
        path.join("metadata.json").exists()
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if Self::exists(&path.to_path_buf()) {
            log::debug!("Found metadata.json , assuming gnome-shell extension.");
            Some(Box::new(Self::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

impl BuildSystem for GnomeShellExtension {
    fn name(&self) -> &str {
        "gnome-shell-extension"
    }

    fn dist(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &std::path::Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn test(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Ok(())
    }

    fn build(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Ok(())
    }

    fn clean(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn install(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(
            crate::buildsystem::DependencyCategory,
            Box<dyn crate::dependency::Dependency>,
        )>,
        crate::buildsystem::Error,
    > {
        let f = std::fs::File::open(self.path.join("metadata.json")).unwrap();

        let metadata: Metadata = serde_json::from_reader(f).unwrap();

        let deps: Vec<(DependencyCategory, Box<dyn crate::dependency::Dependency>)> = vec![(
            DependencyCategory::Universal,
            Box::new(VagueDependency::new(
                "gnome-shell",
                Some(&metadata.shell_version),
            )),
        )];

        Ok(deps)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }
}
