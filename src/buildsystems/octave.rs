//! Support for GNU Octave build systems.
//!
//! This module provides functionality for building, testing, and installing
//! GNU Octave packages.

use crate::buildsystem::{BuildSystem, Error};
use crate::dependencies::octave::OctavePackageDependency;
use crate::dependency::Dependency;
use crate::session::Session;
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// GNU Octave build system.
///
/// This build system handles GNU Octave package builds and installations.
pub struct Octave {
    path: PathBuf,
}

#[allow(dead_code)]
/// Version information for an Octave package.
///
/// Represents a semantic version with major, minor, and patch components.
pub struct Version {
    major: u32,
    minor: u32,
    patch: u32,
}

impl std::str::FromStr for Version {
    type Err = std::num::ParseIntError;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        let mut parts = s.splitn(3, '.');
        let major = parts.next().unwrap().parse()?;
        let minor = parts.next().unwrap().parse()?;
        let patch = parts.next().unwrap().parse()?;
        Ok(Self {
            major,
            minor,
            patch,
        })
    }
}

#[derive(Default)]
/// Metadata for an Octave package.
///
/// Contains the package information from the DESCRIPTION file, including
/// name, version, dependencies, and other metadata.
pub struct Description {
    name: Option<String>,
    version: Option<Version>,
    description: Option<String>,
    date: Option<String>,
    author: Option<String>,
    maintainer: Option<String>,
    title: Option<String>,
    categories: Option<Vec<String>>,
    problems: Option<Vec<String>>,
    url: Option<Vec<url::Url>>,
    depends: Option<Vec<String>>,
    license: Option<String>,
    system_requirements: Option<Vec<String>>,
    build_requires: Option<Vec<String>>,
}

fn read_description_fields<R: std::io::BufRead>(
    r: R,
) -> Result<Vec<(String, String)>, std::io::Error> {
    let mut fields = Vec::new();
    let mut lines = r.lines();
    let line = lines.next().unwrap()?;
    loop {
        if line.is_empty() {
            break;
        }
        if line.starts_with('#') {
            continue;
        }
        let mut parts = line.splitn(2, ": ");
        let key = parts.next().unwrap().to_string();
        let mut value = parts.next().unwrap().to_string();
        while let Some(line) = lines.next() {
            let line = line?;
            if line.starts_with(' ') {
                value.push_str(line.trim_start());
            } else if line.starts_with('#') {
            } else {
                fields.push((key, value));
                break;
            }
        }
    }
    Ok(fields)
}

/// Read an Octave package description from a reader.
///
/// Parses the DESCRIPTION file format used by Octave packages.
///
/// # Arguments
/// * `r` - A BufRead implementation containing the DESCRIPTION file contents
///
/// # Returns
/// The parsed Description struct or an IO error
pub fn read_description<R: std::io::BufRead>(r: R) -> Result<Description, std::io::Error> {
    let mut description = Description::default();
    for (key, value) in read_description_fields(r)?.into_iter() {
        match key.as_str() {
            "Package" => description.name = Some(value),
            "Version" => description.version = Some(value.parse().unwrap()),
            "Description" => description.description = Some(value),
            "Date" => description.date = Some(value),
            "Author" => description.author = Some(value),
            "Maintainer" => description.maintainer = Some(value),
            "Title" => description.title = Some(value),
            "Categories" => {
                description.categories =
                    Some(value.split(',').map(|s| s.trim().to_string()).collect())
            }
            "Problems" => {
                description.problems =
                    Some(value.split(',').map(|s| s.trim().to_string()).collect())
            }
            "URL" => {
                description.url = Some(
                    value
                        .split(',')
                        .map(|s| s.trim().to_string())
                        .map(|s| s.parse().unwrap())
                        .collect::<Vec<url::Url>>(),
                )
            }
            "Depends" => {
                description.depends = Some(value.split(',').map(|s| s.trim().to_string()).collect())
            }
            "License" => description.license = Some(value),
            "SystemRequirements" => {
                description.system_requirements =
                    Some(value.split(',').map(|s| s.trim().to_string()).collect())
            }
            "BuildRequires" => {
                description.build_requires =
                    Some(value.split(',').map(|s| s.trim().to_string()).collect())
            }
            name => log::warn!("Unknown field in DESCRIPTION: {}", name),
        }
    }
    Ok(description)
}

impl Octave {
    /// Create a new Octave build system with the specified path.
    ///
    /// # Arguments
    /// * `path` - The path to the Octave package directory
    ///
    /// # Returns
    /// A new Octave build system instance
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    /// Check if an Octave package exists at the given path.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// `true` if an Octave package exists at the path, `false` otherwise
    pub fn exists(path: &Path) -> bool {
        if path.join("DESCRIPTION").exists() {
            return false;
        }
        // Urgh, isn't there a better way to see if this is an octave package?
        for entry in path.read_dir().unwrap() {
            let entry = entry.unwrap();
            if entry.file_name().to_string_lossy().ends_with(".m") {
                return true;
            }
            if !entry.file_type().unwrap().is_dir() {
                continue;
            }
            match entry.path().read_dir() {
                Ok(subentries) => {
                    for subentry in subentries {
                        let subentry = subentry.unwrap();
                        if subentry.file_name().to_string_lossy().ends_with(".m") {
                            return true;
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    log::debug!(
                        "Permission denied while reading directory: {}",
                        entry.path().display()
                    );
                }
                Err(e) => {
                    panic!("Error reading directory: {}", e);
                }
            }
        }
        false
    }

    /// Probe a directory for an Octave build system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// An Octave build system if one exists at the path, `None` otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if Self::exists(path) {
            log::debug!("Found DESCRIPTION, assuming octave package.");
            Some(Box::new(Self::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

impl BuildSystem for Octave {
    fn name(&self) -> &str {
        "octave"
    }

    fn dist(
        &self,
        _session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn test(
        &self,
        _session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn build(
        &self,
        _session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn clean(
        &self,
        _session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn install(
        &self,
        _session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(crate::buildsystem::DependencyCategory, Box<dyn Dependency>)>,
        crate::buildsystem::Error,
    > {
        let f = std::fs::File::open(self.path.join("DESCRIPTION")).unwrap();
        let description = read_description(std::io::BufReader::new(f)).unwrap();

        let mut ret: Vec<(crate::buildsystem::DependencyCategory, Box<dyn Dependency>)> =
            Vec::new();

        for depend in description.depends.unwrap_or_default() {
            let d: OctavePackageDependency = depend.parse().unwrap();
            ret.push((crate::buildsystem::DependencyCategory::Build, Box::new(d)));
        }

        for build_require in description.build_requires.unwrap_or_default() {
            let d: OctavePackageDependency = build_require.parse().unwrap();
            ret.push((crate::buildsystem::DependencyCategory::Build, Box::new(d)));
        }

        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
