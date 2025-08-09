use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error};
use crate::dependencies::vague::VagueDependency;
use crate::dependency::Dependency;
use crate::dist_catcher::DistCatcher;
use crate::fix_build::BuildFixer;
use crate::installer::Error as InstallerError;
use crate::session::Session;
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// Meson build system.
///
/// Handles projects built with Meson and Ninja.
pub struct Meson {
    #[allow(dead_code)]
    path: PathBuf,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct MesonDependency {
    pub name: String,
    pub version: Vec<String>,
    pub required: bool,
    pub has_fallback: bool,
    pub conditional: bool,
}

#[derive(Debug, serde::Deserialize)]
#[allow(dead_code)]
struct MesonTarget {
    r#type: String,
    installed: bool,
    filename: Vec<PathBuf>,
}

impl Meson {
    /// Create a new Meson build system with the specified path.
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    fn setup(&self, session: &dyn Session) -> Result<(), Error> {
        // Get the project directory (parent of meson.build)
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        let build_dir = project_dir.join("build");

        if !session.exists(&build_dir) {
            session.mkdir(&build_dir).unwrap();
        }
        session
            .command(vec!["meson", "setup", "build"])
            .cwd(project_dir)
            .run_detecting_problems()?;
        Ok(())
    }

    fn introspect<T: for<'a> serde::Deserialize<'a>>(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
        args: &[&str],
    ) -> Result<T, InstallerError> {
        // Get the project directory (parent of meson.build)
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        let args = [&["meson", "introspect"], args, &["./meson.build"]].concat();
        let ret = if let Some(fixers) = fixers {
            session
                .command(args)
                .cwd(project_dir)
                .quiet(true)
                .run_fixing_problems::<_, Error>(fixers)
                .unwrap()
        } else {
            session
                .command(args)
                .cwd(project_dir)
                .run_detecting_problems()?
        };

        let text = ret.concat();

        Ok(serde_json::from_str(&text).unwrap())
    }

    /// Probe a directory for a Meson build system.
    ///
    /// Returns a Meson build system if a meson.build file is found.
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        let path = path.join("meson.build");
        if path.exists() {
            log::debug!("Found meson.build, assuming meson package.");
            Some(Box::new(Self::new(&path)))
        } else {
            None
        }
    }
}

impl BuildSystem for Meson {
    fn name(&self) -> &str {
        "meson"
    }

    fn dist(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        let dc = DistCatcher::new(vec![
            session.external_path(&project_dir.join("build/meson-dist"))
        ]);
        match session
            .command(vec!["ninja", "-C", "build", "dist"])
            .cwd(project_dir)
            .run_detecting_problems()
        {
            Ok(_) => {}
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"ninja: error: unknown target 'dist', did you mean 'dino'?".to_string(),
                ) =>
            {
                unimplemented!();
            }
            Err(e) => return Err(e.into()),
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "test"])
            .cwd(project_dir)
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build"])
            .cwd(project_dir)
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "clean"])
            .cwd(project_dir)
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), Error> {
        self.setup(session)?;
        let project_dir = self
            .path
            .parent()
            .expect("meson.build should have a parent directory");
        session
            .command(vec!["ninja", "-C", "build", "install"])
            .cwd(project_dir)
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn Dependency>)>, Error> {
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = Vec::new();
        let resp =
            self.introspect::<Vec<MesonDependency>>(session, fixers, &["--scan-dependencies"])?;
        for entry in resp {
            let mut minimum_version = None;
            if entry.version.len() == 1 {
                if let Some(rest) = entry.version[0].strip_prefix(">=") {
                    minimum_version = Some(rest.trim().to_string());
                }
            } else if entry.version.len() > 1 {
                log::warn!("Unable to parse version constraints: {:?}", entry.version);
            }
            // TODO(jelmer): Include entry['required']
            ret.push((
                DependencyCategory::Universal,
                Box::new(VagueDependency {
                    name: entry.name.to_string(),
                    minimum_version,
                }),
            ));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        let mut ret: Vec<Box<dyn crate::output::Output>> = Vec::new();
        let resp = self.introspect::<Vec<MesonTarget>>(session, fixers, &["--targets"])?;
        for entry in resp {
            if !entry.installed {
                continue;
            }
            if entry.r#type == "executable" {
                for p in entry.filename {
                    ret.push(Box::new(crate::output::BinaryOutput::new(
                        p.file_name().unwrap().to_str().unwrap(),
                    )));
                }
            }
            // TODO(jelmer): Handle other types
        }

        Ok(ret)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}
