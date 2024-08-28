use crate::session::Session;
use crate::dependency::Dependency;
use crate::installer::{Installer, InstallationScope};
use crate::buildsystem::{BuildSystem, Error};
use std::path::{Path,PathBuf};
use std::os::unix::fs::PermissionsExt;

pub struct Gradle {
    path: PathBuf,
    executable: String,
}

impl Gradle {
    pub fn new(path: PathBuf, executable: String) -> Self {
        Self { path, executable }
    }

    pub fn simple(path: PathBuf) -> Self {
        Self {
            path,
            executable: "gradle".to_string(),
        }
    }

    pub fn exists(path: &Path) -> bool {
        path.join("build.gradle").exists() || path.join("build.gradle.kts").exists()
    }

    pub fn from_path(path: &Path) -> Self {
        if path.join("gradlew").exists() {
            Self::new(path.to_path_buf(), "./gradlew".to_string())
        } else {
            Self::simple(path.to_path_buf())
        }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if Self::exists(path) {
            log::debug!("Found build.gradle, assuming gradle package.");
            Some(Box::new(Self::from_path(path)))
        } else {
            None
        }
    }

    fn setup(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        if !self.executable.starts_with("./") {
            let binary_req = crate::dependencies::BinaryDependency::new(&self.executable);
            if !binary_req.present(session) {
                installer.install(&binary_req, InstallationScope::Global)?;
            }
        }
        Ok(())
    }

    fn run(&self, session: &dyn Session, installer: &dyn Installer, task: &str, args: Vec<&str>) -> Result<(), Error> {
        self.setup(session, installer)?;
        let mut argv = vec![];
        if self.executable.starts_with("./") && (
            !std::fs::metadata(self.path.join(&self.executable)).unwrap().permissions().mode() & 0o111 != 0
        ) {
            argv.push("sh".to_string());
        }
        argv.extend(vec![self.executable.clone(), task.to_owned()]);
        argv.extend(args.iter().map(|x| x.to_string()));
        match session.command(argv.iter().map(|x| x.as_str()).collect()).run_detecting_problems() {
            Err(crate::analyze::AnalyzedError::Unidentified { lines, .. }) if lines.iter().any(|l| lazy_regex::regex_is_match!(r"Task '(.*)' not found in root project '.*'\.", l)) => {
                unimplemented!("Task not found");
            }
            other => other
        }.map(|_| ()).map_err(|e| e.into())
    }
}

impl BuildSystem for Gradle {
    fn name(&self) -> &str {
        "gradle"
    }

    fn dist(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        let dc = crate::dist_catcher::DistCatcher::new(vec![session.external_path(Path::new("."))]);
        self.run(session, installer, "distTar", [].to_vec())?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        self.run(session, installer, "test", [].to_vec())?;
        Ok(())
    }

    fn build(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        self.run(session, installer, "build", [].to_vec())?;
        Ok(())
    }

    fn clean(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        self.run(session, installer, "clean", [].to_vec())?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), crate::buildsystem::Error> {
        unimplemented!();
        // TODO(jelmer): installDist just creates files under build/install/...
        self.run(session, installer, "installDist", [].to_vec())?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, crate::buildsystem::Error> {
        todo!()
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        todo!()
    }
}


