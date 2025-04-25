use crate::dependencies::BinaryDependency;
use crate::dependency::Dependency;
use crate::installer::{
    install_missing_deps, Error as InstallerError, InstallationScope, Installer,
};
use crate::output::Output;
use crate::session::{which, Session};
use std::path::{Path, PathBuf};

/// The category of a dependency
#[derive(Debug, Clone, PartialEq, Eq, std::hash::Hash)]
pub enum DependencyCategory {
    /// A dependency that is required for the package to build and run
    Universal,
    /// Building of artefacts
    Build,
    /// For running artefacts after build or install
    Runtime,
    /// Test infrastructure, e.g. test frameworks or test runners
    Test,
    /// Needed for development, e.g. linters or IDE plugins
    Dev,
    /// Extra build dependencies, e.g. for optional features
    BuildExtra(String),
    /// Extra dependencies, e.g. for optional features
    RuntimeExtra(String),
}

impl std::fmt::Display for DependencyCategory {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            DependencyCategory::Universal => write!(f, "universal"),
            DependencyCategory::Build => write!(f, "build"),
            DependencyCategory::Runtime => write!(f, "runtime"),
            DependencyCategory::Test => write!(f, "test"),
            DependencyCategory::Dev => write!(f, "dev"),
            DependencyCategory::BuildExtra(s) => write!(f, "build-extra:{}", s),
            DependencyCategory::RuntimeExtra(s) => write!(f, "runtime-extra:{}", s),
        }
    }
}

#[derive(Debug)]
/// Error types for build system operations.
///
/// These represent different kinds of errors that can occur when working with build systems.
pub enum Error {
    /// The build system could not be detected.
    NoBuildSystemDetected,

    /// Error occurred while installing dependencies.
    DependencyInstallError(InstallerError),

    /// Error detected and analyzed from build output.
    Error(crate::analyze::AnalyzedError),

    /// Error from an IO operation.
    IoError(std::io::Error),

    /// The requested operation is not implemented by this build system.
    Unimplemented,

    /// Generic error with a message.
    Other(String),
}

impl From<InstallerError> for Error {
    fn from(e: InstallerError) -> Self {
        Error::DependencyInstallError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<crate::analyze::AnalyzedError> for Error {
    fn from(e: crate::analyze::AnalyzedError) -> Self {
        Error::Error(e)
    }
}

impl From<crate::session::Error> for Error {
    fn from(e: crate::session::Error) -> Self {
        match e {
            crate::session::Error::CalledProcessError(e) => {
                crate::analyze::AnalyzedError::Unidentified {
                    retcode: e.code().unwrap(),
                    lines: Vec::new(),
                    secondary: None,
                }
                .into()
            }
            crate::session::Error::IoError(e) => e.into(),
            crate::session::Error::SetupFailure(_, _) => unreachable!(),
        }
    }
}

impl From<crate::fix_build::IterateBuildError<InstallerError>> for Error {
    fn from(e: crate::fix_build::IterateBuildError<InstallerError>) -> Self {
        match e {
            crate::fix_build::IterateBuildError::FixerLimitReached(n) => {
                Error::Other(format!("Fixer limit reached: {}", n))
            }
            crate::fix_build::IterateBuildError::Persistent(e) => {
                crate::analyze::AnalyzedError::Detailed {
                    error: e,
                    retcode: 1,
                }
                .into()
            }
            crate::fix_build::IterateBuildError::Unidentified {
                retcode,
                lines,
                secondary,
            } => crate::analyze::AnalyzedError::Unidentified {
                retcode,
                lines,
                secondary,
            }
            .into(),
            crate::fix_build::IterateBuildError::Other(o) => o.into(),
        }
    }
}

impl From<crate::fix_build::IterateBuildError<Error>> for Error {
    fn from(e: crate::fix_build::IterateBuildError<Error>) -> Self {
        match e {
            crate::fix_build::IterateBuildError::FixerLimitReached(n) => {
                Error::Other(format!("Fixer limit reached: {}", n))
            }
            crate::fix_build::IterateBuildError::Persistent(e) => {
                crate::analyze::AnalyzedError::Detailed {
                    error: e,
                    retcode: 1,
                }
                .into()
            }
            crate::fix_build::IterateBuildError::Unidentified {
                retcode,
                lines,
                secondary,
            } => crate::analyze::AnalyzedError::Unidentified {
                retcode,
                lines,
                secondary,
            }
            .into(),
            crate::fix_build::IterateBuildError::Other(o) => o,
        }
    }
}

impl From<Error> for crate::fix_build::InterimError<Error> {
    fn from(e: Error) -> Self {
        match e {
            Error::Error(crate::analyze::AnalyzedError::Detailed { error, retcode: _ }) => {
                crate::fix_build::InterimError::Recognized(error)
            }
            e => crate::fix_build::InterimError::Other(e),
        }
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoBuildSystemDetected => write!(f, "No build system detected"),
            Error::DependencyInstallError(e) => write!(f, "Error installing dependency: {}", e),
            Error::Error(e) => write!(f, "Error: {}", e),
            Error::IoError(e) => write!(f, "IO Error: {}", e),
            Error::Other(e) => write!(f, "Error: {}", e),
            Error::Unimplemented => write!(f, "Unimplemented"),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
/// Target configuration for installation.
///
/// Defines where and how packages should be installed.
pub struct InstallTarget {
    /// The scope of installation (e.g., global or user).
    pub scope: InstallationScope,

    /// Optional installation prefix directory.
    pub prefix: Option<PathBuf>,
}

impl DependencyCategory {
    /// Get all standard dependency categories.
    ///
    /// Returns an array containing all standard dependency categories.
    pub fn all() -> [DependencyCategory; 5] {
        [
            DependencyCategory::Universal,
            DependencyCategory::Build,
            DependencyCategory::Runtime,
            DependencyCategory::Test,
            DependencyCategory::Dev,
        ]
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
/// Standard build system actions.
///
/// These represent the common actions that can be performed by build systems.
pub enum Action {
    /// Clean the build environment.
    Clean,
    /// Build the project.
    Build,
    /// Run the project's tests.
    Test,
    /// Install the project.
    Install,
}

/// Determine the path to a binary, installing it if necessary
pub fn guaranteed_which(
    session: &dyn Session,
    installer: &dyn Installer,
    name: &str,
) -> Result<PathBuf, InstallerError> {
    match which(session, name) {
        Some(path) => Ok(PathBuf::from(path)),
        None => {
            installer.install(&BinaryDependency::new(name), InstallationScope::Global)?;
            Ok(PathBuf::from(which(session, name).unwrap()))
        }
    }
}

/// A particular buildsystem.
pub trait BuildSystem: std::fmt::Debug {
    /// The name of the buildsystem.
    fn name(&self) -> &str;

    /// Create a distribution package for the project.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    /// * `target_directory` - Directory where the distribution package should be created
    /// * `quiet` - Whether to suppress output
    ///
    /// # Returns
    /// * The filename of the created distribution package on success
    /// * Error if the distribution creation fails
    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error>;

    /// Install the dependencies declared by the build system.
    ///
    /// # Arguments
    /// * `categories` - The categories of dependencies to install
    /// * `scopes` - The scopes in which to install the dependencies
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    /// * `fixers` - Optional list of fixers to use if getting dependency information fails
    ///
    /// # Returns
    /// * `Ok(())` if the dependencies were installed successfully
    /// * Error if installation fails
    fn install_declared_dependencies(
        &self,
        categories: &[DependencyCategory],
        scopes: &[InstallationScope],
        session: &dyn Session,
        installer: &dyn Installer,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<(), Error> {
        let declared_deps = self.get_declared_dependencies(session, fixers)?;
        let relevant = declared_deps
            .into_iter()
            .filter(|(c, _d)| categories.contains(c))
            .map(|(_, d)| d)
            .collect::<Vec<_>>();
        log::debug!("Declared dependencies: {:?}", relevant);
        install_missing_deps(
            session,
            installer,
            scopes,
            relevant
                .iter()
                .map(|d| d.as_ref())
                .collect::<Vec<_>>()
                .as_slice(),
        )?;
        Ok(())
    }

    /// Run tests for the project.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// * `Ok(())` if the tests pass
    /// * Error if the tests fail
    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    /// Build the project.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// * `Ok(())` if the build succeeds
    /// * Error if the build fails
    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    /// Clean the project's build artifacts.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    ///
    /// # Returns
    /// * `Ok(())` if the clean succeeds
    /// * Error if the clean fails
    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    /// Install the project.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `installer` - Installer to use for installing dependencies
    /// * `install_target` - Target configuration for the installation
    ///
    /// # Returns
    /// * `Ok(())` if the installation succeeds
    /// * Error if the installation fails
    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &InstallTarget,
    ) -> Result<(), Error>;

    /// Get the dependencies declared by the build system.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `fixers` - Optional list of fixers to use if getting dependency information fails
    ///
    /// # Returns
    /// * List of dependencies with their categories
    /// * Error if getting dependency information fails
    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error>;

    /// Get the outputs declared by the build system.
    ///
    /// # Arguments
    /// * `session` - The session to run commands in
    /// * `fixers` - Optional list of fixers to use if getting output information fails
    ///
    /// # Returns
    /// * List of declared outputs
    /// * Error if getting output information fails
    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error>;

    /// Convert this build system to Any for dynamic casting.
    ///
    /// This method allows for conversion of the build system to concrete types at runtime.
    ///
    /// # Returns
    /// A reference to this build system as Any
    fn as_any(&self) -> &dyn std::any::Any;
}

/// XML namespaces used by PEAR package definitions.
pub const PEAR_NAMESPACES: &[&str] = &[
    "http://pear.php.net/dtd/package-2.0",
    "http://pear.php.net/dtd/package-2.1",
];

#[derive(Debug)]
/// PEAR (PHP Extension and Application Repository) build system.
pub struct Pear(pub PathBuf);

impl Pear {
    /// Create a new PEAR build system.
    ///
    /// # Arguments
    /// * `path` - Path to the PEAR package.xml file
    ///
    /// # Returns
    /// A new PEAR build system instance
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// Detect if a directory contains a PEAR project.
    ///
    /// # Arguments
    /// * `path` - Directory to probe
    ///
    /// # Returns
    /// * `Some(Box<dyn BuildSystem>)` if a PEAR project is detected
    /// * `None` if no PEAR project is detected
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let package_xml_path = path.join("package.xml");
        if !package_xml_path.exists() {
            return None;
        }

        use xmltree::Element;

        let root = Element::parse(std::fs::File::open(package_xml_path).unwrap()).unwrap();

        // Check that the root tag is <package> and that the namespace is one of the known PEAR
        // namespaces.

        if root
            .namespace
            .as_deref()
            .and_then(|ns| PEAR_NAMESPACES.iter().find(|&n| *n == ns))
            .is_none()
        {
            log::warn!(
                "Namespace of package.xml is not recognized as a PEAR package: {:?}",
                root.namespace
            );
            return None;
        }

        if root.name != "package" {
            log::warn!("Root tag of package.xml is not <package>: {:?}", root.name);
            return None;
        }

        log::debug!(
            "Found package.xml with namespace {}, assuming pear package.",
            root.namespace.as_ref().unwrap()
        );

        Some(Box::new(Self(PathBuf::from(path))))
    }
}

impl BuildSystem for Pear {
    fn name(&self) -> &str {
        "pear"
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        let dc = crate::dist_catcher::DistCatcher::new(vec![session.external_path(Path::new("."))]);
        let pear = guaranteed_which(session, installer, "pear")?;
        session
            .command(vec![pear.to_str().unwrap(), "package"])
            .quiet(quiet)
            .run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session
            .command(vec![pear.to_str().unwrap(), "run-tests"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session
            .command(vec![
                pear.to_str().unwrap(),
                "build",
                self.0.to_str().unwrap(),
            ])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        _install_target: &InstallTarget,
    ) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session
            .command(vec![
                pear.to_str().unwrap(),
                "install",
                self.0.to_str().unwrap(),
            ])
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        let path = self.0.join("package.xml");
        use xmltree::Element;
        let root = Element::parse(std::fs::File::open(path).unwrap()).unwrap();

        // Check that the root tag is <package> and that the namespace is one of the known PEAR
        // namespaces.

        if root
            .namespace
            .as_deref()
            .and_then(|ns| PEAR_NAMESPACES.iter().find(|&n| *n == ns))
            .is_none()
        {
            log::warn!(
                "Namespace of package.xml is not recognized as a PEAR package: {:?}",
                root.namespace
            );
            return Ok(vec![]);
        }

        if root.name != "package" {
            log::warn!("Root tag of package.xml is not <package>: {:?}", root.name);
            return Ok(vec![]);
        }

        let dependencies_tag = root
            .get_child("dependencies")
            .ok_or_else(|| Error::Other("No <dependencies> tag found in <package>".to_string()))?;

        let required_tag = dependencies_tag
            .get_child("required")
            .ok_or_else(|| Error::Other("No <required> tag found in <dependencies>".to_string()))?;

        Ok(required_tag
            .children
            .iter()
            .filter_map(|x| x.as_element())
            .filter(|c| c.name.as_str() == "package")
            .filter_map(
                |package_tag| -> Option<(DependencyCategory, Box<dyn Dependency>)> {
                    let name = package_tag
                        .get_child("name")
                        .and_then(|n| n.get_text())
                        .unwrap()
                        .into_owned();
                    let min_version = package_tag
                        .get_child("min")
                        .and_then(|m| m.get_text())
                        .map(|s| s.into_owned());
                    let max_version = package_tag
                        .get_child("max")
                        .and_then(|m| m.get_text())
                        .map(|s| s.into_owned());
                    let channel = package_tag
                        .get_child("channel")
                        .and_then(|c| c.get_text())
                        .map(|s| s.into_owned());

                    Some((
                        DependencyCategory::Universal,
                        Box::new(crate::dependencies::php::PhpPackageDependency {
                            package: name,
                            channel,
                            min_version,
                            max_version,
                        }) as Box<dyn Dependency>,
                    ))
                },
            )
            .collect())
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Detect build systems.
pub fn scan_buildsystems(path: &Path) -> Vec<(PathBuf, Box<dyn BuildSystem>)> {
    let mut ret = vec![];
    ret.extend(
        detect_buildsystems(path)
            .into_iter()
            .map(|bs| (PathBuf::from(path), bs)),
    );

    if ret.is_empty() {
        // Nothing found. Try the next level?
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                ret.extend(
                    detect_buildsystems(&entry.path())
                        .into_iter()
                        .map(|bs| (entry.path(), bs)),
                );
            }
        }
    }

    ret
}

#[derive(Debug)]
/// PHP Composer build system.
pub struct Composer(pub PathBuf);

impl Composer {
    /// Create a new Composer build system instance.
    ///
    /// # Arguments
    /// * `path` - Path to the project directory
    ///
    /// # Returns
    /// A new Composer build system instance
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// Detect if a directory contains a Composer project.
    ///
    /// # Arguments
    /// * `path` - Directory to probe
    ///
    /// # Returns
    /// * `Some(Box<dyn BuildSystem>)` if a Composer project is detected
    /// * `None` if no Composer project is detected
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("composer.json").exists() {
            Some(Box::new(Self(path.into())))
        } else {
            None
        }
    }
}

impl BuildSystem for Composer {
    fn name(&self) -> &str {
        "composer"
    }

    fn dist(
        &self,
        _session: &dyn Session,
        _installer: &dyn Installer,
        _target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        todo!()
    }

    fn test(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn build(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn clean(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        _session: &dyn Session,
        _installer: &dyn Installer,
        _install_target: &InstallTarget,
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
/// Generic build system that just runs tests.
pub struct RunTests(pub PathBuf);

impl RunTests {
    /// Create a new RunTests build system instance.
    ///
    /// # Arguments
    /// * `path` - Path to the project directory
    ///
    /// # Returns
    /// A new RunTests build system instance
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    /// Detect if a directory contains a project with tests that can be run.
    ///
    /// # Arguments
    /// * `path` - Directory to probe
    ///
    /// # Returns
    /// * `Some(Box<dyn BuildSystem>)` if runnable tests are detected
    /// * `None` if no runnable tests are detected
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("runtests.sh").exists() {
            Some(Box::new(Self(path.into())))
        } else {
            None
        }
    }
}

impl BuildSystem for RunTests {
    fn name(&self) -> &str {
        "runtests"
    }

    fn dist(
        &self,
        _session: &dyn Session,
        _installer: &dyn Installer,
        _target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        todo!()
    }

    fn test(&self, session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        let interpreter = crate::shebang::shebang_binary(&self.0.join("runtests.sh")).unwrap();
        let argv = if interpreter.is_some() {
            vec!["./runtests.sh"]
        } else {
            vec!["/bin/bash", "./runtests.sh"]
        };

        session.command(argv).run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn clean(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        _session: &dyn Session,
        _installer: &dyn Installer,
        _install_target: &InstallTarget,
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Detect all applicable build systems for a given path.
///
/// This function attempts to detect any build systems that can be used with the
/// provided project directory. Multiple build systems may be detected for a single project.
///
/// # Arguments
/// * `path` - Path to the project directory
///
/// # Returns
/// A vector of detected build systems, sorted in order of preference
pub fn detect_buildsystems(path: &std::path::Path) -> Vec<Box<dyn BuildSystem>> {
    if !path.exists() {
        log::error!("Path does not exist: {:?}", path);
        return vec![];
    }
    let path = path.canonicalize().unwrap();
    let mut ret = vec![];
    for probe in [
        Pear::probe,
        crate::buildsystems::python::SetupPy::probe,
        crate::buildsystems::node::Node::probe,
        crate::buildsystems::waf::Waf::probe,
        crate::buildsystems::ruby::Gem::probe,
        crate::buildsystems::meson::Meson::probe,
        crate::buildsystems::rust::Cargo::probe,
        crate::buildsystems::haskell::Cabal::probe,
        crate::buildsystems::java::Gradle::probe,
        crate::buildsystems::java::Maven::probe,
        crate::buildsystems::perl::DistZilla::probe,
        crate::buildsystems::perl::PerlBuildTiny::probe,
        crate::buildsystems::go::Golang::probe,
        crate::buildsystems::bazel::Bazel::probe,
        crate::buildsystems::r::R::probe,
        crate::buildsystems::octave::Octave::probe,
        crate::buildsystems::make::CMake::probe,
        crate::buildsystems::gnome::GnomeShellExtension::probe,
        // Make is intentionally at the end of the list.
        crate::buildsystems::make::Make::probe,
        Composer::probe,
        RunTests::probe,
    ] {
        let bs = probe(&path);
        if let Some(bs) = bs {
            ret.push(bs);
        }
    }
    ret
}

/// Get the most appropriate build system for a given path.
///
/// This function returns the first (most preferred) build system that can be used
/// with the provided project directory, along with its path.
///
/// # Arguments
/// * `path` - Path to the project directory
///
/// # Returns
/// An optional tuple containing the path to the build system file and the build system instance
pub fn get_buildsystem(path: &Path) -> Option<(PathBuf, Box<dyn BuildSystem>)> {
    scan_buildsystems(path).into_iter().next()
}

/// Get a build system by name for a given path.
///
/// This function tries to create a specific build system by name for the provided
/// project directory.
///
/// # Arguments
/// * `name` - Name of the build system to use
/// * `path` - Path to the project directory
///
/// # Returns
/// An optional build system instance if the specified build system is applicable
pub fn buildsystem_by_name(name: &str, path: &Path) -> Option<Box<dyn BuildSystem>> {
    match name {
        "pear" => Pear::probe(path),
        "composer" => Composer::probe(path),
        "runtests" => RunTests::probe(path),
        "setup.py" => crate::buildsystems::python::SetupPy::probe(path),
        "node" => crate::buildsystems::node::Node::probe(path),
        "waf" => crate::buildsystems::waf::Waf::probe(path),
        "gem" => crate::buildsystems::ruby::Gem::probe(path),
        "meson" => crate::buildsystems::meson::Meson::probe(path),
        "cargo" => crate::buildsystems::rust::Cargo::probe(path),
        "cabal" => crate::buildsystems::haskell::Cabal::probe(path),
        "gradle" => crate::buildsystems::java::Gradle::probe(path),
        "maven" => crate::buildsystems::java::Maven::probe(path),
        "distzilla" => crate::buildsystems::perl::DistZilla::probe(path),
        "perl-build-tiny" => crate::buildsystems::perl::PerlBuildTiny::probe(path),
        "go" => crate::buildsystems::go::Golang::probe(path),
        "bazel" => crate::buildsystems::bazel::Bazel::probe(path),
        "r" => crate::buildsystems::r::R::probe(path),
        "octave" => crate::buildsystems::octave::Octave::probe(path),
        "cmake" => crate::buildsystems::make::CMake::probe(path),
        "gnome-shell-extension" => crate::buildsystems::gnome::GnomeShellExtension::probe(path),
        "make" => crate::buildsystems::make::Make::probe(path),
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::installer::NullInstaller;
    use crate::session::plain::PlainSession;

    #[test]
    fn test_guaranteed_which() {
        let session = PlainSession::new();
        let installer = NullInstaller::new();

        let _path = guaranteed_which(&session, &installer, "ls").unwrap();
    }

    #[test]
    fn test_guaranteed_which_not_found() {
        let session = PlainSession::new();
        let installer = NullInstaller::new();

        assert!(matches!(
            guaranteed_which(&session, &installer, "this-does-not-exist").unwrap_err(),
            InstallerError::UnknownDependencyFamily,
        ));
    }
}
