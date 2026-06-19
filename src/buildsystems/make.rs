use crate::analyze::AnalyzedError;
use crate::buildsystem::{BuildSystem, DependencyCategory, Error, InstallTarget};
use crate::dependency::Dependency;
use crate::installer::Installer;
use crate::session::Session;
use crate::shebang::shebang_binary;
use std::path::{Path, PathBuf};

/// Make variable assignments that disable autotools regeneration.
///
/// A tree shipped with an older automake series bakes a versioned
/// aclocal/automake into Makefile.in, and without AM_MAINTAINER_MODE the
/// auto-regen rules fire on a bare make and invoke e.g. aclocal-1.16, which is
/// not installed. We build the sources as shipped rather than regenerate the
/// build system, so point the regen tools at a no-op.
pub const NO_REGEN_MAKE_VARS: &[&str] = &[
    "ACLOCAL=:",
    "AUTOCONF=:",
    "AUTOMAKE=:",
    "AUTOHEADER=:",
    "MAKEINFO=:",
];

#[derive(Debug, Copy, Clone, PartialEq, Eq)]
enum Kind {
    MakefilePL,
    Automake,
    Autoconf,
    Qmake,
    Make,
}

#[derive(Debug)]
/// Make build system.
///
/// Supports different kinds of Make-based build systems, including regular Make,
/// Automake, Autoconf, Makefile.PL, and Qmake.
pub struct Make {
    path: PathBuf,
    kind: Kind,
}

/// Check if a Makefile exists in the current directory.
fn makefile_exists(session: &dyn Session) -> bool {
    session.exists(Path::new("Makefile"))
        || session.exists(Path::new("GNUmakefile"))
        || session.exists(Path::new("makefile"))
}

impl Make {
    /// Create a new Make build system with the specified path.
    ///
    /// Automatically detects the specific type of Make build system.
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
        // `self.path` is the project's host-side path (detection runs on the
        // external dir), used for host filesystem reads like `shebang_binary`.
        // Commands and `session.exists`/`read_dir` operate inside the session,
        // which is already chdir'd to the project, so they use the session's cwd
        // (its in-session project dir) rather than the host path; under an
        // unshare session the two differ and mixing them silently skips the
        // configure steps (and panics in the qmake `read_dir`).
        let sdir = session.pwd().to_path_buf();
        if self.kind == Kind::MakefilePL && !makefile_exists(session) {
            session
                .command(vec!["perl", "Makefile.PL"])
                .cwd(&sdir)
                .run_detecting_problems()?;
        }

        if !makefile_exists(session) && !session.exists(&sdir.join("configure")) {
            if session.exists(&sdir.join("autogen.sh")) {
                if shebang_binary(&self.path.join("autogen.sh"))
                    .unwrap()
                    .is_none()
                {
                    session
                        .command(vec!["/bin/sh", "./autogen.sh"])
                        .cwd(&sdir)
                        .run_detecting_problems()?;
                }
                match session
                    .command(vec!["./autogen.sh"])
                    .cwd(&sdir)
                    .run_detecting_problems()
                {
                    Err(AnalyzedError::Unidentified { lines, .. })
                        if lines.contains(
                            &"Gnulib not yet bootstrapped; run ./bootstrap instead.".to_string(),
                        ) =>
                    {
                        session
                            .command(vec!["./bootstrap"])
                            .cwd(&sdir)
                            .run_detecting_problems()?;
                        session
                            .command(vec!["./autogen.sh"])
                            .cwd(&sdir)
                            .run_detecting_problems()
                    }
                    other => other,
                }?;
            } else if session.exists(&sdir.join("configure.ac"))
                || session.exists(&sdir.join("configure.in"))
            {
                session
                    .command(vec!["autoreconf", "-i"])
                    .cwd(&sdir)
                    .run_detecting_problems()?;
            }
        }

        if !makefile_exists(session) && session.exists(&sdir.join("configure")) {
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
                .cwd(&sdir)
                .run_detecting_problems()?;
        }

        if !makefile_exists(session)
            && session
                .read_dir(&sdir)
                .unwrap()
                .iter()
                .any(|n| n.file_name().to_str().unwrap().ends_with(".pro"))
        {
            session
                .command(vec!["qmake"])
                .cwd(&sdir)
                .run_detecting_problems()?;
        }

        Ok(())
    }

    /// Run the configure step (autogen.sh/autoreconf, then ./configure) so that a
    /// Makefile exists, without building anything.
    ///
    /// Autotools source trees ship only `configure.ac`/`Makefile.am` and have no
    /// `Makefile` until configured, so a bare `make` fails. Callers that drive
    /// `make` directly (e.g. the SCIP indexer wrapping it in `bear`) use this to
    /// prepare the tree first. No-op once a Makefile is present.
    pub fn configure(&self, session: &dyn Session, installer: &dyn Installer) -> Result<(), Error> {
        self.setup(session, installer, None)
    }

    /// Whether this is a Perl project driven by a `Makefile.PL` (ExtUtils::MakeMaker).
    pub fn is_makefile_pl(&self) -> bool {
        self.kind == Kind::MakefilePL
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

        let build_path = self.path.join("build");
        let cwd = if session.exists(&build_path.join("Makefile")) {
            &build_path
        } else {
            &self.path
        };

        let args = [vec!["make"], args, NO_REGEN_MAKE_VARS.to_vec()].concat();

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
                    .cwd(&self.path)
                    .run_detecting_problems()?;
                session.command(args).cwd(cwd).run_detecting_problems()
            }
            Err(AnalyzedError::Unidentified { lines, .. })
                if lines.contains(
                    &"Reconfigure the source tree (via './config' or 'perl Configure'), please."
                        .to_string(),
                ) =>
            {
                session
                    .command(vec!["./config"])
                    .cwd(&self.path)
                    .run_detecting_problems()?;
                session.command(args).cwd(cwd).run_detecting_problems()
            }
            other => other,
        }
        .map(|_| ())
    }

    /// Probe a directory for a Make build system.
    ///
    /// Returns a Make build system if a Makefile or related build files are found.
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingMakeTarget {
    fn to_dependency(&self) -> Option<Box<dyn crate::dependency::Dependency>> {
        if let Some(_local_path) = self.0.strip_prefix("/<<PKGBUILDDIR>>/") {
            // Local file or target
            None
        } else if self.0.starts_with('/') {
            Some(Box::new(crate::dependencies::PathDependency::from(
                PathBuf::from(&self.0),
            )))
        } else {
            None
        }
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
                    fixers.unwrap_or(&[]),
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug)]
/// CMake build system.
///
/// Handles projects built with CMake, using out-of-source builds.
pub struct CMake {
    path: PathBuf,
    builddir: String,
}

impl CMake {
    /// Create a new CMake build system with the specified path.
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
        let build_path = self.path.join(&self.builddir);
        if !session.exists(&build_path) {
            session.mkdir(&build_path)?;
        }
        match session
            .command(vec!["cmake", ".", &format!("-B{}", self.builddir)])
            .cwd(&self.path)
            .run_detecting_problems()
        {
            Ok(_) => Ok(()),
            Err(e) => {
                session.rmtree(&build_path)?;
                Err(e.into())
            }
        }
    }

    /// Probe a directory for a CMake build system.
    ///
    /// Returns a CMake build system if a CMakeLists.txt file is found.
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
            .cwd(&self.path)
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
            .cwd(&self.path)
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
            .cwd(&self.path)
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

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use test_log::test;

    #[test]
    fn test_exists() {
        let mut session = crate::session::plain::PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        session.chdir(td.path()).unwrap();
        assert!(!makefile_exists(&session));
        std::fs::write(td.path().join("Makefile"), b"").unwrap();
        assert!(makefile_exists(&session));
    }

    #[test]
    fn test_simple() {
        let mut session = crate::session::plain::PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        session.chdir(td.path()).unwrap();
        std::fs::write(
            td.path().join("Makefile"),
            r###"

all:

test:

check:

"###,
        )
        .unwrap();
        let make = Make::probe(td.path()).expect("make");

        make.build(&session, &crate::installer::NullInstaller)
            .unwrap();

        std::mem::drop(td);
    }

    /// An autotools tree (configure.ac, no Makefile) is detected as Autoconf.
    /// This is the case the SCIP indexer must configure before running `make`;
    /// before that fix a bare `make` failed with "no makefile found".
    #[test]
    fn test_configure_ac_detected_as_autoconf() {
        let td = tempfile::tempdir().unwrap();
        std::fs::write(td.path().join("configure.ac"), b"AC_INIT([x],[1])\n").unwrap();
        std::fs::write(td.path().join("Makefile.am"), b"").unwrap();
        assert_eq!(Make::new(td.path()).kind, Kind::Automake);
    }

    /// configure() is a no-op when a Makefile is already present, so it does not
    /// require any autotools toolchain in that case.
    #[test]
    fn test_configure_noop_with_makefile() {
        let mut session = crate::session::plain::PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        session.chdir(td.path()).unwrap();
        std::fs::write(td.path().join("Makefile"), b"all:\n").unwrap();
        Make::new(td.path())
            .configure(&session, &crate::installer::NullInstaller)
            .unwrap();
    }
}
