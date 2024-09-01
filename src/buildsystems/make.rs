use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error, InstallTarget};
use crate::dependency::Dependency;
use crate::installer::Installer;
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

#[derive(Debug)]
pub struct Make {
    path: PathBuf,
    kind: Kind,
}

fn makefile_exists(session: &dyn Session) -> bool {
    session.exists(Path::new("Makefile"))
        || session.exists(Path::new("GNUmakefile"))
        || session.exists(Path::new("makefile"))
}

impl Make {
    pub fn new(path: &Path) -> Self {
        let kind = if path.join("Makefile.PL").exists() {
            Kind::MakefilePL
        } else if path.join("Makefile.am").exists() {
            Kind::Automake
        } else if path.join("configure.ac").exists()
            || path.join("configure.in").exists()
            || path.join("autogen.sh").exists()
        {
            Kind::Autoconf
        } else if path
            .read_dir()
            .unwrap()
            .any(|n| n.unwrap().file_name().to_string_lossy().ends_with(".pro"))
        {
            Kind::Qmake
        } else {
            Kind::Make
        };
        Self {
            path: path.to_path_buf(),
            kind,
        }
    }

    fn setup(
        &self,
        session: &dyn Session,
        _installer: &dyn Installer,
        prefix: Option<&Path>,
    ) -> Result<(), Error> {
        if self.kind == Kind::MakefilePL && !makefile_exists(session) {
            session
                .command(vec!["perl", "Makefile.PL"])
                .run_detecting_problems()?;
        }

        if !makefile_exists(session) && !session.exists(Path::new("configure")) {
            if session.exists(Path::new("autogen.sh")) {
                if shebang_binary(&self.path.join("autogen.sh"))
                    .unwrap()
                    .is_none()
                {
                    session
                        .command(vec!["/bin/sh", "./autogen.sh"])
                        .run_detecting_problems()?;
                }
                match session
                    .command(vec!["./autogen.sh"])
                    .run_detecting_problems()
                {
                    Err(AnalyzedError::Unidentified { lines, .. })
                        if lines.contains(
                            &"Gnulib not yet bootstrapped; run ./bootstrap instead.".to_string(),
                        ) =>
                    {
                        session
                            .command(vec!["./bootstrap"])
                            .run_detecting_problems()?;
                        session
                            .command(vec!["./autogen.sh"])
                            .run_detecting_problems()
                    }
                    other => other,
                }?;
            } else if session.exists(Path::new("configure.ac"))
                || session.exists(Path::new("configure.in"))
            {
                session
                    .command(vec!["autoreconf", "-i"])
                    .run_detecting_problems()?;
            }
        }

        if !makefile_exists(session) && session.exists(Path::new("configure")) {
            let args = [
                vec!["./configure".to_string()],
                if let Some(p) = prefix {
                    vec![format!("--prefix={}", p.to_str().unwrap())]
                } else {
                    vec![]
                },
            ]
            .concat();
            session
                .command(args.iter().map(|s| s.as_str()).collect())
                .run_detecting_problems()?;
        }

        if !makefile_exists(session)
            && session
                .read_dir(Path::new("."))
                .unwrap()
                .iter()
                .any(|n| n.file_name().to_str().unwrap().ends_with(".pro"))
        {
            session.command(vec!["qmake"]).run_detecting_problems()?;
        }

        Ok(())
    }

    fn run_make(
        &self,
        session: &dyn Session,
        args: Vec<&str>,
        prefix: Option<&Path>,
    ) -> Result<(), AnalyzedError> {
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
            lazy_regex::regex_is_match!(
                r"Makefile:[0-9]+: \*\*\* You need to run \.\/configure .*",
                line
            )
        }

        let cwd = if session.exists(Path::new("build/Makefile")) {
            Path::new("build")
        } else {
            Path::new(".")
        };

        let args = [vec!["make"], args].concat();

        match session
            .command(args.clone())
            .cwd(cwd)
            .run_detecting_problems()
        {
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.len() < 5 && lines.iter().any(|l| wants_configure(l)) =>
            {
                session
                    .command(
                        [
                            vec!["./configure".to_string()],
                            if let Some(p) = prefix.as_ref() {
                                vec![format!("--prefix={}", p.to_str().unwrap())]
                            } else {
                                vec![]
                            },
                        ]
                        .concat()
                        .iter()
                        .map(|x| x.as_str())
                        .collect(),
                    )
                    .run_detecting_problems()?;
                session.command(args).cwd(cwd).run_detecting_problems()
            }
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"Reconfigure the source tree (via './config' or 'perl Configure'), please."
                        .to_string(),
                ) =>
            {
                session.command(vec!["./config"]).run_detecting_problems()?;
                session.command(args).cwd(cwd).run_detecting_problems()
            }
            other => other,
        }
        .map(|_| ())
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if [
            "Makefile",
            "GNUmakefile",
            "makefile",
            "Makefile.PL",
            "autogen.sh",
            "configure.ac",
            "configure.in",
        ]
        .iter()
        .any(|p| path.join(p).exists())
        {
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
        _quiet: bool,
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
                session.command(vec!["make", "manifest"]).run_detecting_problems()?;
                session.command(vec!["make", "dist"]).run_detecting_problems().map(|_| ())
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.iter().any(|l| lazy_regex::regex_is_match!(r"(Makefile|GNUmakefile|makefile):[0-9]+: \*\*\* Missing 'Make.inc' Run './configure \[options\]' and retry.  Stop.", l)) => {
                session.command(vec!["./configure"]).run_detecting_problems()?;
                session.command(vec!["make", "dist"]).run_detecting_problems().map(|_| ())
            }
            Err(AnalyzedError::Unidentified { lines, .. }) if lines.iter().any(|l| lazy_regex::regex_is_match!(r"Problem opening MANIFEST: No such file or directory at .* line [0-9]+\.", l)) => {
                session.command(vec!["make", "manifest"]).run_detecting_problems()?;
                session.command(vec!["make", "dist"]).run_detecting_problems().map(|_| ())
            }
            other => other
        }?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
    ) -> Result<(), Error> {
        self.setup(session, installer, None)?;
        for target in ["check", "test"] {
            match self.run_make(session, vec![target], None) {
                Err(AnalyzedError::Unidentified { lines, .. })
                    if lines.contains(&format!(
                        "make: *** No rule to make target '{}'.  Stop.",
                        target
                    )) =>
                {
                    continue;
                }
                Err(AnalyzedError::Unidentified { lines, .. })
                    if lines.contains(&format!(
                        "make[1]: *** No rule to make target '{}'.  Stop.",
                        target
                    )) =>
                {
                    continue;
                }
                other => other,
            }?;
            return Ok(());
        }

        if self.path.join("t").exists() {
            // See https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
            session
                .command(vec!["prove", "-b", "t/"])
                .run_detecting_problems()?;
        } else {
            log::warn!("No test target found");
        }
        Ok(())
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
    ) -> Result<(), Error> {
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

    fn clean(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
    ) -> Result<(), Error> {
        self.setup(session, installer, None)?;
        self.run_make(session, vec!["clean"], None)?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn Installer,
        install_target: &InstallTarget,
    ) -> Result<(), Error> {
        self.setup(session, installer, install_target.prefix.as_deref())?;
        self.run_make(session, vec!["install"], install_target.prefix.as_deref())?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn crate::session::Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(
            crate::buildsystem::DependencyCategory,
            Box<dyn crate::dependency::Dependency>,
        )>,
        Error,
    > {
        // TODO(jelmer): Split out the perl-specific stuff?
        let mut ret = vec![];
        let meta_yml = self.path.join("META.yml");
        if meta_yml.exists() {
            let mut f = std::fs::File::open(meta_yml).unwrap();
            ret.extend(
                crate::buildsystems::perl::declared_deps_from_meta_yml(&mut f)
                    .into_iter()
                    .map(|d| (d.0, Box::new(d.1) as Box<dyn crate::dependency::Dependency>)),
            );
        }
        let cpanfile = self.path.join("cpanfile");
        if cpanfile.exists() {
            ret.extend(
                crate::buildsystems::perl::declared_deps_from_cpanfile(
                    session,
                    fixers.unwrap_or(&vec![]),
                )
                .into_iter()
                .map(|d| (d.0, Box::new(d.1) as Box<dyn crate::dependency::Dependency>)),
            );
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        Err(Error::Unimplemented)
    }
}

#[derive(Debug)]
pub struct CMake {
    path: PathBuf,
    builddir: String,
}

impl CMake {
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
            builddir: "build".to_string(),
        }
    }

    fn setup(
        &self,
        session: &dyn Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), Error> {
        if !session.exists(Path::new(&self.builddir)) {
            session.mkdir(Path::new(&self.builddir))?;
        }
        match session
            .command(vec!["cmake", ".", &format!("-B{}", self.builddir)])
            .run_detecting_problems()
        {
            Ok(_) => Ok(()),
            Err(e) => {
                session.rmtree(Path::new(&self.builddir))?;
                Err(e.into())
            }
        }
    }

    pub fn probe(path: &Path) -> Option<Box<dyn crate::buildsystem::BuildSystem>> {
        if path.join("CMakeLists.txt").exists() {
            return Some(Box::new(Self::new(path)));
        }
        None
    }
}

impl crate::buildsystem::BuildSystem for CMake {
    fn name(&self) -> &str {
        "cmake"
    }

    fn dist(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &std::path::Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        Err(crate::buildsystem::Error::Unimplemented)
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        session
            .command(vec!["cmake", "--build", &self.builddir])
            .run_detecting_problems()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        session
            .command(vec!["cmake", "--install", &self.builddir])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        self.setup(session, installer)?;
        session
            .command(vec![
                "cmake",
                "--build",
                &self.builddir,
                ".",
                "--target",
                "clean",
            ])
            .run_detecting_problems()?;
        Ok(())
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
        // TODO(jelmer): Find a proper parser for CMakeLists.txt somewhere?
        use std::io::BufRead;
        let f = std::fs::File::open(self.path.join("CMakeLists.txt")).unwrap();
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        for line in std::io::BufReader::new(f).lines() {
            let line = line.unwrap();
            if let Some((_, m)) = lazy_regex::regex_captures!(
                r"cmake_minimum_required\(\s*VERSION\s+(.*)\s*\)",
                &line
            ) {
                ret.push((
                    crate::buildsystem::DependencyCategory::Build,
                    Box::new(crate::dependencies::vague::VagueDependency::new(
                        "CMake",
                        Some(m),
                    )),
                ));
            }
        }

        Ok(ret)
    }

    fn test(&self, _session: &dyn Session, _installer: &dyn Installer) -> Result<(), Error> {
        Err(Error::Unimplemented)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, Error> {
        Err(Error::Unimplemented)
    }
}
