use crate::dependencies::BinaryDependency;
use crate::dependency::Dependency;
use crate::output::Output;
use crate::installer::{Error as InstallerError, InstallationScope, Installer, install_missing_deps};
use crate::session::{which, Session};
use std::path::{Path, PathBuf};

/// The category of a dependency
#[derive(Debug, Clone, PartialEq, Eq)]
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
}

#[derive(Debug)]
pub enum Error {
    /// The build system could not be detected.
    NoBuildSystemDetected,

    DependencyInstallError(InstallerError),

    Error(crate::analyze::AnalyzedError),

    Other(String),
}

impl From<InstallerError> for Error {
    fn from(e: InstallerError) -> Self {
        Error::DependencyInstallError(e)
    }
}

impl From<crate::analyze::AnalyzedError> for Error {
    fn from(e: crate::analyze::AnalyzedError) -> Self {
        Error::Error(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::NoBuildSystemDetected => write!(f, "No build system detected"),
            Error::DependencyInstallError(e) => write!(f, "Error installing dependency: {}", e),
            Error::Error(e) => write!(f, "Error: {}", e),
            Error::Other(e) => write!(f, "Error: {}", e),
        }
    }
}

impl std::error::Error for Error {}

#[derive(Debug, Clone)]
pub struct InstallTarget {
    pub scope: InstallationScope,

    pub prefix: Option<PathBuf>,
}

impl DependencyCategory {
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
pub enum Action {
    Clean,
    Build,
    Test,
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
pub trait BuildSystem {
    /// The name of the buildsystem.
    fn name(&self) -> &str;

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error>;

    fn install_declared_dependencies(
        &self,
        categories: &[DependencyCategory],
        scope: InstallationScope,
        session: &dyn Session,
        installer: &dyn Installer,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<(), Error> {
        let declared_deps = self.get_declared_dependencies(session, fixers)?;
        let relevant =
            declared_deps.into_iter().filter(|(c, _d)| categories.contains(c)).map(|(_, d)| d).collect::<Vec<_>>();
        install_missing_deps(session, installer, scope, relevant.iter().map(|d| d.as_ref()).collect::<Vec<_>>().as_slice())?;
        Ok(())
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error>;

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &InstallTarget
    ) -> Result<(), Error>;

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error>;

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error>;
}

pub const PEAR_NAMESPACES: &[&str] = &[
    "http://pear.php.net/dtd/package-2.0",
    "http://pear.php.net/dtd/package-2.1",
];

pub struct Pear(pub PathBuf);

impl Pear {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let package_xml_path = path.join("package.xml");
        if !package_xml_path.exists() {
            return None;
        }

        use xmltree::Element;

        let root = Element::parse(std::fs::File::open(package_xml_path).unwrap()).unwrap();

        // Check that the root tag is <package> and that the namespace is one of the known PEAR
        // namespaces.

        if root.namespace.as_deref().and_then(|ns| PEAR_NAMESPACES.iter().find(|&n| *n == ns)).is_none() {
            log::warn!("Namespace of package.xml is not recognized as a PEAR package: {:?}", root.namespace);
            return None;
        }

        if root.name != "package" {
            log::warn!("Root tag of package.xml is not <package>: {:?}", root.name);
            return None;
        }

        log::debug!("Found package.xml with namespace {}, assuming pear package.", root.namespace.as_ref().unwrap());

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
        session.command(vec![pear.to_str().unwrap(), "package"]).quiet(quiet).run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session.command(vec![pear.to_str().unwrap(), "run-tests"]).run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session.command(vec![pear.to_str().unwrap(), "build", self.0.to_str().unwrap()]).run_detecting_problems()?;
        Ok(())
    }

    fn clean(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        _install_target: &InstallTarget
    ) -> Result<(), Error> {
        let pear = guaranteed_which(session, installer, "pear")?;
        session.command(vec![pear.to_str().unwrap(), "install", self.0.to_str().unwrap()]).run_detecting_problems()?;
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

        if root.namespace.as_deref().and_then(|ns| PEAR_NAMESPACES.iter().find(|&n| *n == ns)).is_none() {
            log::warn!("Namespace of package.xml is not recognized as a PEAR package: {:?}", root.namespace);
            return Ok(vec![]);
        }

        if root.name != "package" {
            log::warn!("Root tag of package.xml is not <package>: {:?}", root.name);
            return Ok(vec![]);
        }

        let dependencies_tag = root.get_child("dependencies").ok_or_else(|| {
            Error::Other("No <dependencies> tag found in <package>".to_string())
        })?;

        let required_tag = dependencies_tag.get_child("required").ok_or_else(|| {
            Error::Other("No <required> tag found in <dependencies>".to_string())
        })?;

        Ok(required_tag.children.iter().filter_map(|x| x.as_element()).filter(|c| c.name.as_str() == "package").filter_map(|package_tag| -> Option<(DependencyCategory, Box<dyn Dependency>)> {
            let name = package_tag.get_child("name").and_then(|n| n.get_text()).unwrap().into_owned();
            let min_version = package_tag.get_child("min").and_then(|m| m.get_text()).map(|s| s.into_owned());
            let max_version = package_tag.get_child("max").and_then(|m| m.get_text()).map(|s| s.into_owned());
            let channel = package_tag.get_child("channel").and_then(|c| c.get_text()).map(|s| s.into_owned());

            Some((DependencyCategory::Universal, Box::new(crate::dependencies::php::PhpPackageDependency {
                package: name,
                channel,
                min_version,
                max_version,
            }) as Box<dyn Dependency>))
        }).collect())
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }
}

/// Detect build systems.
pub fn scan_buildsystems(path: &Path) -> Vec<(PathBuf, Box<dyn BuildSystem>)> {
    let mut ret = vec![];
    ret.extend(detect_buildsystems(path).map(|bs| (PathBuf::from(path), bs)));

    if ret.is_empty() {
        // Nothing found. Try the next level?
        for entry in std::fs::read_dir(path).unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_dir() {
                ret.extend(
                    detect_buildsystems(&entry.path()).map(|bs| (entry.path(), bs))
                );
            }
        }
    }

    ret
}
pub struct Composer(pub PathBuf);

impl Composer {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

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
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        todo!()
    }

    fn test(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &InstallTarget
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }
}

pub struct RunTests(pub PathBuf);

impl RunTests {
    pub fn new(path: PathBuf) -> Self {
        Self(path)
    }

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
        session: &dyn Session,
        installer: &dyn Installer,
        target_directory: &Path,
        quiet: bool,
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

    fn build(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn clean(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn Installer,
        install_target: &InstallTarget
    ) -> Result<(), Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn Dependency>)>, Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn Output>>, Error> {
        todo!()
    }

}



pub fn detect_buildsystems(path: &std::path::Path) -> Option<Box<dyn BuildSystem>> {
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
        /*,
        PerlBuildTiny::probe,
        Golang::probe,
        R::probe,
        Octave::probe,
        Bazel::probe,
        CMake::probe,
        GnomeShellExtension::probe,
        */
        /* Make is intentionally at the end of the list. */
        crate::buildsystems::make::Make::probe,
        Composer::probe,
        RunTests::probe,
        ] {
        let bs = probe(path);
        if let Some(bs) = bs {
            return Some(bs);
        }
    }
    None
}

pub fn get_buildsystem(path: &Path) -> Option<(PathBuf, Box<dyn BuildSystem>)> {
    scan_buildsystems(path).into_iter().next()
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
