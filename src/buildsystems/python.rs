//! Support for Python build systems.
//!
//! This module provides functionality for building, testing, and installing
//! Python packages using various build systems such as setuptools, poetry, and pip.

use crate::analyze::{run_detecting_problems, AnalyzedError};
use crate::buildsystem::{BuildSystem, DependencyCategory, Error, InstallTarget};
use crate::dependencies::python::{PythonDependency, PythonPackageDependency};
use crate::dependency::Dependency;
use crate::dist_catcher::DistCatcher;
use crate::fix_build::BuildFixer;
use crate::installer::{Error as InstallerError, InstallationScope, Installer};
use crate::output::{BinaryOutput, Output, PythonPackageOutput};
use crate::session::Session;
use pyo3::exceptions::{
    PyFileNotFoundError, PyImportError, PyModuleNotFoundError, PyRuntimeError, PySystemExit,
};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde::Deserialize;
use std::collections::{HashMap, HashSet};
use std::io::Seek;
use std::path::{Path, PathBuf};
use toml;

#[derive(Debug, Deserialize)]
#[allow(dead_code)]
struct Distribution {
    name: Option<String>,
    requires: Vec<String>,
    setup_requires: Vec<String>,
    install_requires: Vec<String>,
    tests_require: Vec<String>,
    scripts: Vec<String>,
    packages: Vec<String>,
    entry_points: HashMap<String, Vec<String>>,
}

fn load_toml(path: &Path) -> Result<pyproject_toml::PyProjectToml, PyErr> {
    let path = path.join("pyproject.toml");
    let text = std::fs::read_to_string(path).unwrap();

    Ok(toml::from_str(&text).unwrap())
}

#[derive(Debug)]
/// A wrapper around a Python setup.cfg configuration file.
///
/// This provides access to the configuration in a setup.cfg file, which is used
/// by setuptools to configure Python package builds.
pub struct SetupCfg(PyObject);

impl SetupCfg {
    fn has_section(&self, section: &str) -> bool {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "__contains__", (section,))
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        })
    }

    fn get_section(&self, section: &str) -> Option<SetupCfgSection> {
        Python::with_gil(|py| {
            if self.has_section(section) {
                let section: Option<PyObject> = self
                    .0
                    .call_method1(py, "get", (section, py.None()))
                    .unwrap()
                    .extract(py)
                    .ok();
                Some(SetupCfgSection(section.unwrap()))
            } else {
                None
            }
        })
    }
}

/// A section in a Python setup.cfg configuration file.
///
/// This provides access to a specific section in a setup.cfg file, allowing
/// access to configuration keys within that section.
pub struct SetupCfgSection(PyObject);

impl Default for SetupCfg {
    fn default() -> Self {
        Python::with_gil(|py| SetupCfg(py.None()))
    }
}

impl SetupCfgSection {
    fn get<T: for<'a> FromPyObject<'a>>(&self, key: &str) -> Option<T> {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "get", (key, py.None()))
                .unwrap()
                .extract::<Option<T>>(py)
                .unwrap()
        })
    }

    /// Check if a key exists in this section.
    ///
    /// # Arguments
    /// * `key` - The key to check for
    ///
    /// # Returns
    /// `true` if the key exists, `false` otherwise
    pub fn has_key(&self, key: &str) -> bool {
        Python::with_gil(|py| {
            self.0
                .call_method1(py, "__contains__", (key,))
                .unwrap()
                .extract::<bool>(py)
                .unwrap()
        })
    }
}

fn load_setup_cfg(path: &Path) -> Result<Option<SetupCfg>, PyErr> {
    Python::with_gil(|py| {
        let m = py.import("setuptools.config.setupcfg")?;
        let read_configuration = m.getattr("read_configuration")?;

        let p = path.join("setup.cfg");

        if p.exists() {
            let config = read_configuration.call1((p,))?;
            Ok(Some(SetupCfg(config.unbind())))
        } else {
            Ok(None)
        }
    })
}

//  run_setup, but setting __name__
// Imported from Python's distutils.core, Copyright (C) PSF

fn run_setup(py: Python, script_name: &Path, stop_after: &str) -> PyResult<PyObject> {
    assert!(
        stop_after == "init"
            || stop_after == "config"
            || stop_after == "commandline"
            || stop_after == "run"
    );
    // Import setuptools, just in case it decides to replace distutils
    let _ = py.import("setuptools");

    let core = match py.import("distutils.core") {
        Ok(m) => m,
        Err(e) if e.is_instance_of::<PyImportError>(py) => {
            // Importing distutils failed, but that's fine.
            match py.import("setuptools._distutils.core") {
                Ok(m) => m,
                Err(e) => return Err(e),
            }
        }
        Err(e) => return Err(e),
    };

    core.setattr("_setup_stop_after", stop_after)?;

    let sys = py.import("sys")?;
    let os = py.import("os")?;

    let save_argv = sys.getattr("argv")?;

    let g = PyDict::new(py);
    g.set_item("__file__", script_name)?;
    g.set_item("__name__", "__main")?;

    let old_cwd = os.getattr("getcwd")?.call0()?.extract::<String>()?;
    os.call_method1(
        "chdir",
        (os.getattr("path")?
            .call_method1("dirname", (script_name,))?,),
    )?;

    sys.setattr("argv", vec![script_name])?;

    let text = std::fs::read_to_string(script_name)?;

    let code = std::ffi::CString::new(text).unwrap();
    let r = py.eval(&code, Some(&g), None);

    os.call_method1("chdir", (old_cwd,))?;
    sys.setattr("argv", save_argv)?;
    core.setattr("_setup_stop_after", py.None())?;

    match r {
        Ok(_) => Ok(core.getattr("_setup_distribution")?.unbind()),
        Err(e) if e.is_instance_of::<PySystemExit>(py) => {
            Ok(core.getattr("_setup_distribution")?.unbind())
        }
        Err(e) => Err(e),
    }
}

const SETUP_WRAPPER: &str = r#"""
try:
    import setuptools
except ImportError:
    pass
import distutils
from distutils import core
import sys

import os
script_name = "%(script_name)s"
os.chdir(os.path.dirname(script_name))

g = {"__file__": os.path.basename(script_name), "__name__": "__main__"}
try:
    core._setup_stop_after = "init"
    sys.argv[0] = script_name
    with open(script_name, "rb") as f:
        exec(f.read(), g)
except SystemExit:
    # Hmm, should we do something if exiting with a non-zero code
    # (ie. error)?
    pass

if core._setup_distribution is None:
    raise RuntimeError(
        (
            "'distutils.core.setup()' was never called -- "
            "perhaps '%s' is not a Distutils setup script?"
        )
        % script_name
    )

d = core._setup_distribution
r = {
    'name': getattr(d, "name", None) or None,
    'setup_requires': getattr(d, "setup_requires", []),
    'install_requires': getattr(d, "install_requires", []),
    'tests_require': getattr(d, "tests_require", []) or [],
    'scripts': getattr(d, "scripts", []) or [],
    'entry_points': getattr(d, "entry_points", None) or {},
    'packages': getattr(d, "packages", []) or [],
    'requires': d.get_requires() or [],
    }
import os
import json
with open(%(output_path)s, 'w') as f:
    json.dump(r, f)
"""#;

#[derive(Debug)]
/// A Python setuptools-based build system.
///
/// This build system handles Python packages that use setup.py for building,
/// which is the traditional approach for Python packages.
pub struct SetupPy {
    path: PathBuf,
    has_setup_py: bool,
    config: Option<SetupCfg>,
    pyproject: Option<pyproject_toml::PyProjectToml>,
    #[allow(dead_code)]
    buildsystem: Option<String>,
}

impl SetupPy {
    /// Create a new SetupPy build system with the specified path.
    ///
    /// This will load and parse setup.cfg and pyproject.toml if they exist.
    ///
    /// # Arguments
    /// * `path` - The path to the Python project directory
    ///
    /// # Returns
    /// A new SetupPy build system instance
    pub fn new(path: &Path) -> Self {
        let has_setup_py = path.join("setup.py").exists();

        Python::with_gil(|py| {
            let config = match load_setup_cfg(path) {
                Ok(config) => config,
                Err(e) if e.is_instance_of::<PyFileNotFoundError>(py) => None,
                Err(e) if e.is_instance_of::<PyModuleNotFoundError>(py) => {
                    log::warn!("Error parsing setup.cfg: {}", e);
                    None
                }
                Err(e) => {
                    panic!("Error parsing setup.cfg: {}", e);
                }
            };

            let (pyproject, buildsystem) = match load_toml(path) {
                Ok(pyproject) => {
                    let buildsystem = pyproject
                        .build_system
                        .as_ref()
                        .and_then(|bs| bs.build_backend.clone());
                    (Some(pyproject), buildsystem)
                }
                Err(e) if e.is_instance_of::<PyFileNotFoundError>(py) => (None, None),
                Err(e) => {
                    panic!("Error parsing pyproject.toml: {}", e);
                }
            };

            Self {
                has_setup_py,
                path: path.to_owned(),
                config,
                pyproject,
                buildsystem,
            }
        })
    }

    /// Probe a directory for a Python setuptools build system.
    ///
    /// # Arguments
    /// * `path` - The path to check
    ///
    /// # Returns
    /// A SetupPy build system if one exists at the path, `None` otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("setup.py").exists() {
            log::debug!("Found setup.py, assuming python project.");
            return Some(Box::new(Self::new(path)));
        }
        if path.join("pyproject.toml").exists() {
            log::debug!("Found pyproject.toml, assuming python project.");
            return Some(Box::new(Self::new(path)));
        }
        None
    }

    fn extract_setup_direct(&self) -> Option<Distribution> {
        let p = self.path.join("setup.py").canonicalize().unwrap();

        Python::with_gil(|py| {
            let d = match run_setup(py, &p, "init") {
                Err(e) if e.is_instance_of::<PyRuntimeError>(py) => {
                    log::warn!("Unable to load setup.py metadata: {}", e);
                    return None;
                }
                Ok(d) => d,
                Err(e) => {
                    panic!("Unable to load setup.py metadata: {}", e);
                }
            };

            let name: Option<String> = d.getattr(py, "name").unwrap().extract(py).unwrap();
            let setup_requires: Vec<String> = d
                .call_method1(py, "get", ("setup_requires", Vec::<String>::new()))
                .unwrap()
                .extract(py)
                .unwrap();
            let install_requires: Vec<String> = d
                .call_method1(py, "get", ("install_requires", Vec::<String>::new()))
                .unwrap()
                .extract(py)
                .unwrap();
            let tests_require: Vec<String> = d
                .call_method1(py, "get", ("tests_require", Vec::<String>::new()))
                .unwrap()
                .extract(py)
                .unwrap();
            let scripts: Vec<String> = d
                .call_method1(py, "get", ("scripts", Vec::<String>::new()))
                .unwrap()
                .extract(py)
                .unwrap();
            let entry_points: HashMap<String, Vec<String>> = d
                .call_method1(
                    py,
                    "get",
                    ("entry_points", HashMap::<String, Vec<String>>::new()),
                )
                .unwrap()
                .extract(py)
                .unwrap();
            let packages: Vec<String> = d
                .call_method1(py, "get", ("packages", Vec::<String>::new()))
                .unwrap()
                .extract(py)
                .unwrap();
            let requires: Vec<String> = d
                .call_method0(py, "get_requires")
                .unwrap()
                .extract(py)
                .unwrap();

            Some(Distribution {
                name,
                setup_requires,
                install_requires,
                tests_require,
                scripts,
                entry_points,
                packages,
                requires,
            })
        })
    }

    fn determine_interpreter(&self) -> String {
        if let Some(config) = self.config.as_ref() {
            let python_requires: Option<String> = config
                .get_section("options")
                .and_then(|s| s.get::<String>("python_requires"));
            if python_requires
                .map(|pr| !pr.contains("2.7"))
                .unwrap_or(true)
            {
                return "python3".to_owned();
            }
        }
        let path = self.path.join("setup.py");
        let shebang_binary = crate::shebang::shebang_binary(&path).unwrap();

        shebang_binary.unwrap_or("python3".to_owned())
    }

    fn extract_setup_in_session(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
    ) -> Option<Distribution> {
        let interpreter = self.determine_interpreter();

        let mut output_f = tempfile::NamedTempFile::new_in(session.location().join("tmp")).unwrap();
        let argv: Vec<String> = vec![
            interpreter,
            "-c".to_string(),
            SETUP_WRAPPER
                .replace(
                    "%(script_name)s",
                    session.pwd().join("setup.py").to_str().unwrap(),
                )
                .replace(
                    "%(output_path)s",
                    &format!(
                        "\"/{}\"",
                        output_f
                            .path()
                            .to_str()
                            .unwrap()
                            .strip_prefix(session.location().to_str().unwrap())
                            .unwrap()
                    ),
                ),
        ];
        let r = if let Some(fixers) = fixers {
            session
                .command(argv.iter().map(|x| x.as_str()).collect::<Vec<_>>())
                .quiet(true)
                .run_fixing_problems::<_, Error>(fixers)
                .map(|_| ())
                .map_err(|e| e.to_string())
        } else {
            session
                .command(argv.iter().map(|x| x.as_str()).collect())
                .check_call()
                .map_err(|e| e.to_string())
        };
        match r {
            Ok(_) => (),
            Err(e) => {
                log::warn!("Unable to load setup.py metadata: {}", e);
                return None;
            }
        }
        output_f.seek(std::io::SeekFrom::Start(0)).unwrap();
        Some(serde_json::from_reader(output_f).unwrap())
    }

    fn extract_setup(
        &self,
        session: Option<&dyn Session>,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
    ) -> Option<Distribution> {
        if !self.has_setup_py {
            return None;
        }
        if let Some(session) = session {
            self.extract_setup_in_session(session, fixers)
        } else {
            self.extract_setup_direct()
        }
    }

    fn setup_requires(&self) -> Vec<PythonPackageDependency> {
        let mut ret = vec![];
        if let Some(build_system) = self
            .pyproject
            .as_ref()
            .and_then(|p| p.build_system.as_ref())
        {
            let requires = &build_system.requires;
            for require in requires {
                ret.push(PythonPackageDependency::from(require.clone()));
            }
        }

        if let Some(config) = &self.config {
            let options = config.get_section("options");
            let setup_requires = options
                .and_then(|os| os.get::<Vec<String>>("setup_requires"))
                .unwrap_or(vec![]);
            for require in &setup_requires {
                ret.push(PythonPackageDependency::try_from(require.clone()).unwrap());
            }
        }
        ret
    }

    fn run_setup(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        args: Vec<&str>,
    ) -> Result<(), Error> {
        // Install the setup_requires beforehand, since otherwise
        // setuptools might fetch eggs instead of our preferred installer.
        let setup_requires = self
            .setup_requires()
            .into_iter()
            .map(|x| Box::new(x) as Box<dyn Dependency>)
            .collect::<Vec<_>>();
        crate::installer::install_missing_deps(
            session,
            installer,
            &[crate::installer::InstallationScope::Global],
            setup_requires
                .iter()
                .map(|x| x.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        let interpreter = self.determine_interpreter().clone();
        let mut args = args.clone();
        args.insert(0, &interpreter);
        args.insert(1, "setup.py");
        // TODO(jelmer): Perhaps this should be additive?
        session.command(args).run_detecting_problems()?;
        Ok(())
    }
}

impl BuildSystem for SetupPy {
    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        if self.path.join("tox.ini").exists() {
            run_detecting_problems(
                session,
                vec!["tox", "--skip-missing-interpreters"],
                None,
                false,
                None,
                None,
                None,
                None,
            )?;
            return Ok(());
        }
        if self
            .config
            .as_ref()
            .map(|c| c.has_section("tool:pytest") || c.has_section("pytest"))
            .unwrap_or(false)
        {
            session.command(vec!["pytest"]).run_detecting_problems()?;
            return Ok(());
        }
        if self.has_setup_py {
            // Pre-emptively install setuptools, since distutils doesn't provide
            // a 'test' subcommand and some packages fall back to distutils
            // if setuptools is not available.
            let setuptools_dep = PythonPackageDependency::simple("setuptools");
            if !setuptools_dep.present(session) {
                installer.install(&setuptools_dep, InstallationScope::Global)?;
            }
            match self.run_setup(session, installer, vec!["test"]) {
                Ok(_) => {
                    return Ok(());
                }
                Err(Error::Error(AnalyzedError::Unidentified { lines, .. }))
                    if lines.contains(&"error: invalid command 'test'".to_string()) =>
                {
                    return Ok(());
                }
                Err(e) => {
                    return Err(e);
                }
            }
        }
        unimplemented!();
    }

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        if self.has_setup_py {
            self.run_setup(session, installer, vec!["build"])
        } else {
            unimplemented!();
        }
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        // TODO(jelmer): Look at self.build_backend
        let dc = DistCatcher::new(vec![session.external_path(Path::new("dist"))]);
        if self.has_setup_py {
            let mut preargs = vec![];
            if quiet {
                preargs.push("--quiet");
            }
            // Preemptively install setuptools since some packages fail in some way without it.
            let setuptools_req = PythonPackageDependency::simple("setuptools");
            if !setuptools_req.present(session) {
                installer.install(&setuptools_req, InstallationScope::Global)?;
            }
            preargs.push("sdist");
            self.run_setup(session, installer, preargs)?;
        } else if self.pyproject.is_some() {
            run_detecting_problems(
                session,
                vec!["python3", "-m", "build", "--sdist", "."],
                None,
                false,
                None,
                None,
                None,
                None,
            )?;
        } else {
            panic!("No setup.py or pyproject.toml");
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        if self.has_setup_py {
            self.run_setup(session, installer, vec!["clean"])
        } else {
            unimplemented!();
        }
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &InstallTarget,
    ) -> Result<(), Error> {
        if self.has_setup_py {
            let mut args = vec![];
            if install_target.scope == InstallationScope::User {
                args.push("--user".to_string());
            }
            if let Some(prefix) = install_target.prefix.as_ref() {
                args.push(format!("--prefix={}", prefix.to_str().unwrap()));
            }
            args.insert(0, "install".to_owned());
            self.run_setup(
                session,
                installer,
                args.iter().map(|x| x.as_str()).collect(),
            )?;
            Ok(())
        } else {
            unimplemented!();
        }
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: std::option::Option<&[&dyn BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        let distribution = self.extract_setup(Some(session), fixers);
        if let Some(distribution) = distribution {
            for require in &distribution.requires {
                ret.push((
                    DependencyCategory::Universal,
                    Box::new(PythonPackageDependency::try_from(require.clone()).unwrap()),
                ));
            }
            // Not present for distutils-only packages
            for require in &distribution.setup_requires {
                ret.push((
                    DependencyCategory::Build,
                    Box::new(PythonPackageDependency::try_from(require.clone()).unwrap()),
                ));
            }
            // Not present for distutils-only packages
            for require in &distribution.install_requires {
                ret.push((
                    DependencyCategory::Universal,
                    Box::new(PythonPackageDependency::try_from(require.clone()).unwrap()),
                ));
            }
            // Not present for distutils-only packages
            for require in &distribution.tests_require {
                ret.push((
                    DependencyCategory::Test,
                    Box::new(PythonPackageDependency::try_from(require.clone()).unwrap()),
                ));
            }
        }
        if let Some(pyproject) = self.pyproject.as_ref() {
            if let Some(build_system) = pyproject.build_system.as_ref() {
                for require in &build_system.requires {
                    ret.push((
                        DependencyCategory::Build,
                        Box::new(PythonPackageDependency::from(require.clone())),
                    ));
                }
            }
        }
        if let Some(options) = self.config.as_ref().and_then(|c| c.get_section("options")) {
            for require in options
                .get::<Vec<String>>("setup_requires")
                .unwrap_or_default()
            {
                ret.push((
                    DependencyCategory::Build,
                    Box::new(PythonPackageDependency::try_from(require).unwrap()),
                ));
            }
            for require in options
                .get::<Vec<String>>("install_requires")
                .unwrap_or_default()
            {
                ret.push((
                    DependencyCategory::Universal,
                    Box::new(PythonPackageDependency::try_from(require).unwrap()),
                ));
            }
        }

        if let Some(pyproject_toml) = self.pyproject.as_ref() {
            if let Some(build_system) = pyproject_toml.build_system.as_ref() {
                for require in &build_system.requires {
                    ret.push((
                        DependencyCategory::Build,
                        Box::new(PythonPackageDependency::from(require.clone())),
                    ));
                }
            }

            if let Some(dependencies) = pyproject_toml
                .project
                .as_ref()
                .and_then(|p| p.dependencies.as_ref())
            {
                for dep in dependencies {
                    ret.push((
                        DependencyCategory::Universal,
                        Box::new(PythonPackageDependency::from(dep.clone())),
                    ));
                }
            }

            if let Some(extras) = pyproject_toml
                .project
                .as_ref()
                .and_then(|p| p.optional_dependencies.as_ref())
            {
                for (name, deps) in extras {
                    for dep in deps {
                        ret.push((
                            DependencyCategory::RuntimeExtra(name.clone()),
                            Box::new(PythonPackageDependency::from(dep.clone())),
                        ));
                    }
                }
            }

            if let Some(requires_python) = pyproject_toml
                .project
                .as_ref()
                .and_then(|p| p.requires_python.as_ref())
            {
                ret.push((
                    DependencyCategory::Universal,
                    Box::new(PythonDependency::from(requires_python)),
                ));
            }
        }

        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        let mut ret: Vec<Box<dyn Output>> = vec![];
        let distribution = self.extract_setup(Some(session), fixers);
        let mut all_packages = HashSet::new();
        if let Some(distribution) = distribution {
            for script in &distribution.scripts {
                ret.push(Box::new(BinaryOutput(
                    Path::new(script)
                        .file_name()
                        .unwrap()
                        .to_str()
                        .unwrap()
                        .to_owned(),
                )));
            }
            for script in distribution
                .entry_points
                .get("console_scripts")
                .unwrap_or(&vec![])
            {
                ret.push(Box::new(BinaryOutput(
                    script.split_once('=').unwrap().0.to_string().to_owned(),
                )));
            }
            all_packages.extend(distribution.packages);
        }
        if let Some(options) = self.config.as_ref().and_then(|c| c.get_section("options")) {
            all_packages.extend(options.get::<Vec<String>>("packages").unwrap_or_default());
            for script in options.get::<Vec<String>>("scripts").unwrap_or_default() {
                let p = Path::new(&script);
                ret.push(Box::new(BinaryOutput(
                    p.file_name().unwrap().to_str().unwrap().to_owned(),
                )));
            }
            let entry_points = options
                .get::<HashMap<String, Vec<String>>>("entry_points")
                .unwrap_or_default();
            for script in entry_points.get("console_scripts").unwrap_or(&vec![]) {
                ret.push(Box::new(BinaryOutput(
                    script.split_once('=').unwrap().0.to_string().to_owned(),
                )));
            }
        }

        for package in all_packages {
            ret.push(Box::new(PythonPackageOutput::new(
                &package,
                Some("cpython3"),
            )));
        }

        if let Some(pyproject) = self.pyproject.as_ref().and_then(|p| p.project.as_ref()) {
            if let Some(scripts) = pyproject.scripts.as_ref() {
                for (script, _from) in scripts {
                    ret.push(Box::new(BinaryOutput(script.to_string())));
                }
            }

            if let Some(gui_scripts) = pyproject.gui_scripts.as_ref() {
                for (script, _from) in gui_scripts {
                    ret.push(Box::new(BinaryOutput(script.to_string())));
                }
            }

            ret.push(Box::new(PythonPackageOutput::new(
                &pyproject.name,
                pyproject.version.as_ref().map(|v| v.to_string()).as_deref(),
            )));
        }

        Ok(ret)
    }

    fn name(&self) -> &str {
        "setup.py"
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
