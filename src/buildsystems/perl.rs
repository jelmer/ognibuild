//! Support for Perl build systems.
//!
//! This module provides functionality for building, testing, and installing
//! Perl packages using various build systems such as DistZilla, Makefile.PL,
//! and ExtUtils::MakeMaker.

use crate::analyze::AnalyzedError;
use crate::buildsystem::{guaranteed_which, BuildSystem, DependencyCategory, Error};
use crate::dependencies::perl::PerlModuleDependency;
use crate::fix_build::{BuildFixer, IterateBuildError};
use crate::installer::Error as InstallerError;
use crate::session::Session;
use std::collections::HashMap;
use std::io::Read;
use std::path::{Path, PathBuf};

fn read_cpanfile(
    session: &dyn Session,
    args: Vec<&str>,
    category: DependencyCategory,
    fixers: &[&dyn BuildFixer<InstallerError>],
) -> impl Iterator<Item = (DependencyCategory, PerlModuleDependency)> {
    let mut argv = vec!["cpanfile-dump"];
    argv.extend(args);

    session
        .command(argv)
        .run_fixing_problems::<_, crate::buildsystem::Error>(fixers)
        .unwrap()
        .into_iter()
        .filter_map(move |line| {
            let line = line.trim();
            if !line.is_empty() {
                Some((category.clone(), PerlModuleDependency::simple(line)))
            } else {
                None
            }
        })
}

/// Extract declared dependencies from a cpanfile.
///
/// # Arguments
/// * `session` - The session to use for executing commands
/// * `fixers` - Fixers to apply if reading the cpanfile fails
///
/// # Returns
/// A list of dependencies declared in the cpanfile
pub fn declared_deps_from_cpanfile(
    session: &dyn Session,
    fixers: &[&dyn BuildFixer<InstallerError>],
) -> Vec<(DependencyCategory, PerlModuleDependency)> {
    read_cpanfile(
        session,
        vec!["--configure", "--build"],
        DependencyCategory::Build,
        fixers,
    )
    .chain(read_cpanfile(
        session,
        vec!["--test"],
        DependencyCategory::Test,
        fixers,
    ))
    .collect()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
/// Metadata from META.yml for a Perl package.
///
/// This contains the parsed metadata from a META.yml file, which describes
/// the package and its dependencies.
pub struct Meta {
    name: String,
    #[serde(rename = "abstract")]
    r#abstract: String,
    version: String,
    license: String,
    author: Vec<String>,
    distribution_type: String,
    requires: HashMap<String, String>,
    recommends: HashMap<String, String>,
    build_requires: HashMap<String, String>,
    resources: HashMap<String, String>,
    #[serde(rename = "meta-spec")]
    meta_spec: HashMap<String, String>,
    generated_by: String,
    configure_requires: HashMap<String, String>,
}

/// Extract declared dependencies from a META.yml file.
///
/// # Arguments
/// * `f` - A reader for the META.yml file
///
/// # Returns
/// A list of dependencies declared in the META.yml file
pub fn declared_deps_from_meta_yml<R: Read>(
    f: R,
) -> Vec<(DependencyCategory, PerlModuleDependency)> {
    // See http://module-build.sourceforge.net/META-spec-v1.4.html for the specification of the format.
    let data: Meta = serde_yaml::from_reader(f).unwrap();

    let mut ret = vec![];

    // TODO: handle versions
    for name in data.requires.keys() {
        ret.push((
            DependencyCategory::Universal,
            PerlModuleDependency::simple(name),
        ));
    }
    for name in data.build_requires.keys() {
        ret.push((
            DependencyCategory::Build,
            PerlModuleDependency::simple(name),
        ));
    }
    for name in data.configure_requires.keys() {
        ret.push((
            DependencyCategory::Build,
            PerlModuleDependency::simple(name),
        ));
    }
    // TODO(jelmer): recommends
    ret
}

#[derive(Debug)]
/// DistZilla build system for Perl packages.
///
/// This build system handles Perl packages that use DistZilla for building.
pub struct DistZilla {
    path: PathBuf,
    dist_inkt_class: Option<String>,
}

impl DistZilla {
    /// Create a new DistZilla build system with the specified path.
    ///
    /// # Arguments
    /// * `path` - The path to the Perl package directory
    ///
    /// # Returns
    /// A new DistZilla build system instance
    pub fn new(path: PathBuf) -> Self {
        let mut dist_inkt_class = None;
        let mut f = std::fs::File::open(&path).unwrap();
        let mut contents = String::new();
        f.read_to_string(&mut contents).unwrap();
        for line in contents.lines() {
            let rest = if let Some(rest) = line.strip_prefix(";;") {
                rest
            } else {
                continue;
            };
            let (key, value) = if let Some((key, value)) = rest.split_once('=') {
                (key.trim(), value.trim())
            } else {
                continue;
            };
            if key == "class" && value.starts_with("'Dist::Inkt") {
                dist_inkt_class = Some(value[1..value.len() - 1].to_string());
                break;
            }
        }
        Self {
            path,
            dist_inkt_class,
        }
    }

    /// Set up the DistZilla build environment.
    ///
    /// # Arguments
    /// * `installer` - The installer to use for dependencies
    ///
    /// # Returns
    /// Ok on success or an error
    pub fn setup(
        &self,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::installer::Error> {
        let dep = crate::dependencies::perl::PerlModuleDependency::simple("Dist::Inkt");
        installer.install(&dep, crate::installer::InstallationScope::Global)?;
        Ok(())
    }

    /// Probe a directory for a DistZilla build system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// A DistZilla build system if one exists at the path, `None` otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let dist_ini_path = path.join("dist.ini");
        if dist_ini_path.exists() && !path.join("Makefile.PL").exists() {
            Some(Box::new(Self::new(dist_ini_path)))
        } else {
            None
        }
    }
}

impl BuildSystem for DistZilla {
    fn name(&self) -> &str {
        "Dist::Zilla"
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        self.setup(installer)?;
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(Path::new(".")));
        if self.dist_inkt_class.is_some() {
            session
                .command(vec![guaranteed_which(session, installer, "distinkt-dist")
                    .unwrap()
                    .to_str()
                    .unwrap()])
                .quiet(quiet)
                .run_detecting_problems()?;
        } else {
            // Default to invoking Dist::Zilla
            session
                .command(vec![
                    guaranteed_which(session, installer, "dzil")
                        .unwrap()
                        .to_str()
                        .unwrap(),
                    "build",
                    "--tgz",
                ])
                .quiet(quiet)
                .run_detecting_problems()?;
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        // see also https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
        self.setup(installer)?;
        session
            .command(vec![
                guaranteed_which(session, installer, "dzil")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "test",
            ])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(installer)?;
        session
            .command(vec![
                guaranteed_which(session, installer, "dzil")
                    .unwrap()
                    .to_str()
                    .unwrap(),
                "build",
            ])
            .run_detecting_problems()?;
        Ok(())
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
        _installerr: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<
        Vec<(DependencyCategory, Box<dyn crate::dependency::Dependency>)>,
        crate::buildsystem::Error,
    > {
        let mut ret = vec![];
        if self.path.exists() {
            let lines = session
                .command(vec!["dzil", "authordeps"])
                .run_fixing_problems::<_, crate::buildsystem::Error>(fixers.unwrap_or(&[]))
                .unwrap();
            for entry in lines {
                ret.push((
                    DependencyCategory::Build,
                    Box::new(PerlModuleDependency::simple(entry.trim()))
                        as Box<dyn crate::dependency::Dependency>,
                ));
            }
        }
        if self.path.parent().unwrap().join("cpanfile").exists() {
            ret.extend(
                declared_deps_from_cpanfile(session, fixers.unwrap_or(&[]))
                    .into_iter()
                    .map(|(category, dep)| {
                        (
                            category,
                            Box::new(dep) as Box<dyn crate::dependency::Dependency>,
                        )
                    }),
            );
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
/// Module::Build::Tiny build system for Perl packages.
///
/// This build system handles Perl packages that use Module::Build::Tiny for building,
/// including support for Minilla.
pub struct PerlBuildTiny {
    path: PathBuf,
    minilla: bool,
}

impl PerlBuildTiny {
    /// Create a new PerlBuildTiny build system with the specified path.
    ///
    /// # Arguments
    /// * `path` - The path to the Perl package directory
    ///
    /// # Returns
    /// A new PerlBuildTiny build system instance
    pub fn new(path: PathBuf) -> Self {
        let minilla = path.join("minil.toml").exists();
        Self { path, minilla }
    }

    fn setup(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
    ) -> Result<(), crate::buildsystem::Error> {
        let fixers = fixers.unwrap_or(&[]);
        let argv = vec!["perl", "Build.PL"];
        session
            .command(argv)
            .run_fixing_problems::<_, crate::buildsystem::Error>(fixers)?;
        Ok(())
    }

    /// Probe a directory for a Module::Build::Tiny build system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// A PerlBuildTiny build system if one exists at the path, `None` otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("Build.PL").exists() {
            log::debug!("Found Build.PL, assuming Module::Build::Tiny package.");
            Some(Box::new(Self::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

impl BuildSystem for PerlBuildTiny {
    fn name(&self) -> &str {
        "Module::Build::Tiny"
    }

    fn dist(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        self.setup(session, None)?;
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(Path::new(".")));
        if self.minilla {
            // minil seems to return 0 even if it didn't produce a tarball :(
            crate::analyze::run_detecting_problems(
                session,
                vec!["minil", "dist"],
                Some(&|_, _| dc.find_files().is_none()),
                quiet,
                None,
                None,
                None,
                None,
            )?;
        } else {
            match session
                .command(vec!["./Build", "dist"])
                .run_detecting_problems()
            {
                Err(AnalyzedError::Unidentified { lines, .. })
                    if lines.iter().any(|l| {
                        l.contains("Can't find dist packages without a MANIFEST file")
                    }) =>
                {
                    session
                        .command(vec!["./Build", "manifest"])
                        .run_detecting_problems()?;
                    session
                        .command(vec!["./Build", "dist"])
                        .run_detecting_problems()
                }
                Err(AnalyzedError::Unidentified { lines, .. })
                    if lines.iter().any(|l| l.contains("No such action 'dist'")) =>
                {
                    unimplemented!("Module::Build::Tiny dist command not supported");
                }
                other => other,
            }?;
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, None)?;
        if self.minilla {
            session
                .command(vec!["minil", "test"])
                .run_detecting_problems()?;
        } else {
            session
                .command(vec!["./Build", "test"])
                .run_detecting_problems()?;
        }
        Ok(())
    }

    fn build(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, None)?;
        session
            .command(vec!["./Build", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, None)?;
        session
            .command(vec!["./Build", "clean"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, None)?;
        if self.minilla {
            session
                .command(vec!["minil", "install"])
                .run_detecting_problems()?;
        } else {
            session
                .command(vec!["./Build", "install"])
                .run_detecting_problems()?;
        }
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<
        Vec<(DependencyCategory, Box<dyn crate::dependency::Dependency>)>,
        crate::buildsystem::Error,
    > {
        self.setup(session, fixers)?;
        if self.minilla {
            // Minilla doesn't seem to have a way to just regenerate the metadata :(
        } else {
            let cmd = session.command(vec!["./Build", "distmeta"]);

            if let Some(fixers) = fixers {
                match cmd.run_fixing_problems::<_, crate::buildsystem::Error>(fixers) {
                    Err(IterateBuildError::Unidentified { lines, .. })
                        if lines
                            .iter()
                            .any(|l| l.contains("No such action 'distmeta'")) =>
                    {
                        // Module::Build::Tiny doesn't have a distmeta action
                        Ok(Vec::new())
                    }
                    Err(IterateBuildError::Unidentified { lines, .. })
                        if lines.iter().any(|l| {
                            l.contains(
                                "Do not run distmeta. Install Minilla and `minil install` instead.",
                            )
                        }) =>
                    {
                        log::warn!(
                            "did not detect  minilla, but it is required to get the dependencies"
                        );
                        Ok(Vec::new())
                    }
                    other => other,
                }?;
            } else {
                cmd.run_detecting_problems()?;
            }
        }
        let meta_yml_path = self.path.join("META.yml");
        if meta_yml_path.exists() {
            let f = std::fs::File::open(&meta_yml_path).unwrap();
            Ok(declared_deps_from_meta_yml(f)
                .into_iter()
                .map(|(category, dep)| {
                    (
                        category,
                        Box::new(dep) as Box<dyn crate::dependency::Dependency>,
                    )
                })
                .collect())
        } else {
            Ok(vec![])
        }
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
