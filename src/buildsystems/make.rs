use crate::buildsystem::{BuildSystem, Error, InstallTarget};
use crate::installer::{Installer, InstallationScope};
use crate::analyze::{run_detecting_problems, AnalyzedError};
use crate::session::Session;
use crate::shebang::shebang_binary;
use std::path::{Path, PathBuf};

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Kind {
    MakefilePL,
    Automake,
    Autoconf,
    Qmake,
    Make,
}

pub struct Make {
    path: PathBuf,
    kind: Kind,
}

fn makefile_exists(session: &dyn Session) -> bool {
    session.exists(Path::new("Makefile")) || session.exists(Path::new("GNUmakefile")) || session.exists(Path::new("makefile"))
}

impl Make {
    pub fn new(path: &Path) -> Self {
        let kind = if path.join("Makefile.PL").exists() {
            Kind::MakefilePL
        } else if path.join("Makefile.am").exists() {
            Kind::Automake
        } else if path.join("configure.ac").exists() || path.join("configure.in").exists() || path.join("autogen.sh").exists() {
            Kind::Autoconf
        } else if path.read_dir().unwrap().any(|n| n.unwrap().file_name().to_string_lossy().ends_with(".pro")) {
            Kind::Qmake
        } else {
            Kind::Make
        };
        Self { path: path.to_path_buf(), kind }
    }

    fn setup(&self, session: &dyn Session, _installer: &dyn Installer, prefix: Option<&Path>) -> Result<(), Error> {
        if self.kind == Kind::MakefilePL && !makefile_exists(session) {
            run_detecting_problems(session, vec!["perl", "Makefile.PL"], None, false, None, None, None, None, None, None)?;
        }

        if !makefile_exists(session) && !session.exists(Path::new("configure")) {
            if session.exists(Path::new("autogen.sh")) {
                if shebang_binary(&self.path.join("autogen.sh")).unwrap().is_none() {
                    run_detecting_problems(session, vec!["/bin/sh", "./autogen.sh"], None, false, None, None, None, None, None, None)?;
                }
                match run_detecting_problems(session, vec!["./autogen.sh"], None, false, None, None, None, None, None, None) {
                    Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"Gnulib not yet bootstrapped; run ./bootstrap instead.".to_string()) => {
                            run_detecting_problems(session, vec!["./bootstrap"], None, false, None, None, None, None, None, None)?;
                            run_detecting_problems(session, vec!["./autogen.sh"], None, false, None, None, None, None, None, None)
                    }
                    other => other
                }?;
            } else if session.exists(Path::new("configure.ac")) || session.exists(Path::new("configure.in")) {
                run_detecting_problems(session, vec!["autoreconf", "-i"], None, false, None, None, None, None, None, None)?;
            }
        }

        if !makefile_exists(session) && session.exists(Path::new("configure")) {
            let args = [vec!["./configure".to_string()], if let Some(p) = prefix {
                vec![format!("--prefix={}", p.to_str().unwrap())]
            } else {
                vec![]
            }].concat();
            run_detecting_problems(session, args.iter().map(|s| s.as_str()).collect(), None, false, None, None, None, None, None, None)?;
        }

        if !makefile_exists(session) && session.read_dir(Path::new(".")).unwrap().iter().any(|n| n.file_name().to_str().unwrap().ends_with(".pro")) {
            run_detecting_problems(session, vec!["qmake"], None, false, None, None, None, None, None, None)?;
        }

        Ok(())
    }

    fn run_make(&self, session: &dyn Session, args: Vec<&str>, prefix: Option<&Path>) -> Result<(), AnalyzedError> {
        fn wants_configure(line: &str) -> bool {
            if line.starts_with("Run ./configure") {
                return true;
            }
            if line == "Please run ./configure first" {
                return true;
            }
            if line.starts_with("Project not configured") {
                return true;
            }
            if line.starts_with("The project was not configured") {
                return true;
            }
            lazy_regex::regex_is_match!(r"Makefile:[0-9]+: \*\*\* You need to run \.\/configure .*", line)
        }

        let cwd = if session.exists(Path::new("build/Makefile")) {
            Some(Path::new("build"))
        } else {
            None
        };

        let args = [vec!["make"], args].concat();

        match run_detecting_problems(session, args.clone(), None, false, cwd, None, None, None, None, None) {
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.len() < 5 && lines.iter().any(|l| wants_configure(l)) => {
                run_detecting_problems(session, [vec!["./configure".to_string()], if let Some(p) = prefix.as_ref() {
                    vec![format!("--prefix={}", p.to_str().unwrap())]
                } else {
                    vec![]
                }].concat().iter().map(|x| x.as_str()).collect(), None, false, None, None, None, None, None, None)?;
                run_detecting_problems(session, args, None, false, cwd, None, None, None, None, None)
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"Reconfigure the source tree (via './config' or 'perl Configure'), please.".to_string()) => {
                run_detecting_problems(session, vec!["./config"], None, false, None, None, None, None, None, None)?;
                run_detecting_problems(session, args, None, false, cwd, None, None, None, None, None)
            }
            other => other
        }.map(|_| ())
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if ["Makefile", "GNUmakefile", "makefile", "Makefile.PL", "autogen.sh", "configure.ac", "configure.in"].iter().any(|p| path.join(p).exists()) {
            return Some(Box::new(Self::new(path)));
        }
        for n in path.read_dir().unwrap() {
            let n = n.unwrap();
            if n.file_name().to_string_lossy().ends_with(".pro") {
                return Some(Box::new(Self::new(path)));
            }
        }
        None
    }
}

impl BuildSystem for Make {
    fn name(&self) -> &str {
        "make"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
        target_directory: &std::path::Path,
        quiet: bool,
    ) -> Result<std::ffi::OsString, Error> {
        self.setup(session, installer, None)?;
        let dc = crate::dist_catcher::DistCatcher::default(&session.external_path(Path::new(".")));

        match self.run_make(session, vec!["dist"], None) {
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"make: *** No rule to make target 'dist'.  Stop.".to_string()) => {
                unimplemented!();
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"make[1]: *** No rule to make target 'dist'.  Stop.".to_string()) => {
                unimplemented!();
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"ninja: error: unknown target 'dist', did you mean 'dino'?".to_string()) => {
                unimplemented!();
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&"Please try running 'make manifest' and then run 'make dist' again.".to_string()) => {
                run_detecting_problems(session, vec!["make", "manifest"], None, false, None, None, None, None, None, None)?;
                run_detecting_problems(session, vec!["make", "dist"], None, false, None, None, None, None, None, None).map(|_| ())
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.iter().any(|l| lazy_regex::regex_is_match!(r"(Makefile|GNUmakefile|makefile):[0-9]+: \*\*\* Missing 'Make.inc' Run './configure \[options\]' and retry.  Stop.", l)) => {
                run_detecting_problems(session, vec!["./configure"], None, false, None, None, None, None, None, None)?;
                run_detecting_problems(session, vec!["make", "dist"], None, false, None, None, None, None, None, None).map(|_| ())
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.iter().any(|l| lazy_regex::regex_is_match!(r"Problem opening MANIFEST: No such file or directory at .* line [0-9]+\.", l)) => {
                run_detecting_problems(session, vec!["make", "manifest"], None, false, None, None, None, None, None, None)?;
                run_detecting_problems(session, vec!["make", "dist"], None, false, None, None, None, None, None, None).map(|_| ())
            }
            other => other
        }?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn crate::session::Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer, None)?;
        for target in ["check", "test"] {
            match self.run_make(session, vec![target], None) {
                Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&format!("make: *** No rule to make target '{}'.  Stop.", target)) => {
                    continue;
                }
                Err(AnalyzedError::Unidentified { lines, .. }) if lines.contains(&format!("make[1]: *** No rule to make target '{}'.  Stop.", target)) => {
                    continue;
                }
                other => other,
            }?;
            return Ok(());
        }

        if self.path.join("t").exists() {
            // See https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
            run_detecting_problems(session, vec!["prove", "-b", "t/"], None, false, None, None, None, None, None, None)?;
        } else {
            log::warn!("No test target found");
        }
        Ok(())
    }

    fn build(&self, session: &dyn crate::session::Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer, None)?;
        let default_target = match self.kind {
            Kind::Qmake => None,
            _ => Some("all"),
        };
        let args = if let Some(target) = default_target {
            vec![target]
        } else {
            vec![]
        };
        self.run_make(session, args, None)?;
        Ok(())
    }

    fn clean(&self, session: &dyn crate::session::Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer, None)?;
        self.run_make(session, vec!["clean"], None)?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
        install_target: &InstallTarget
    ) -> Result<(), Error> {
        self.setup(session, installer, install_target.prefix.as_deref())?;
        self.run_make(session, vec!["install"], install_target.prefix.as_deref())?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<(crate::buildsystem::DependencyCategory, Box<dyn crate::dependency::Dependency>)>, Error> {
        // TODO(jelmer): Split out the perl-specific stuff?
        let mut ret = vec![];
        let meta_yml = self.path.join("META.yml");
        if meta_yml.exists() {
            let mut f = std::fs::File::open(meta_yml).unwrap();
            ret.extend(crate::buildsystems::perl::declared_deps_from_meta_yml(&mut f).into_iter().map(|d| (d.0, Box::new(d.1) as Box<dyn crate::dependency::Dependency>)));
        }
        let cpanfile = self.path.join("cpanfile");
        if cpanfile.exists() {
            ret.extend(crate::buildsystems::perl::declared_deps_from_cpanfile(session, fixers.unwrap_or(&vec![])).into_iter().map(|d| (d.0, Box::new(d.1) as Box<dyn crate::dependency::Dependency>)));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        todo!()
    }
}
