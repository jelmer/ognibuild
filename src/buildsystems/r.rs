use crate::buildsystem::guaranteed_which;
use crate::buildsystem::{BuildSystem, DependencyCategory};
use crate::dependencies::r::RPackageDependency;
use crate::dependency::Dependency;
use crate::dist_catcher::DistCatcher;
use crate::output::RPackageOutput;
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub struct R {
    path: PathBuf,
}

impl R {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn lint(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        let r_path = guaranteed_which(session, installer, "R").unwrap();
        session
            .command(vec![r_path.to_str().unwrap(), "CMD", "check"])
            .run_detecting_problems()?;
        Ok(())
    }

    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("DESCRIPTION").exists() && path.join("NAMESPACE").exists() {
            Some(Box::new(Self::new(path.to_path_buf())))
        } else {
            None
        }
    }
}

impl BuildSystem for R {
    fn name(&self) -> &str {
        "R"
    }

    fn dist(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        let dc = DistCatcher::new(vec![session.external_path(Path::new("."))]);
        let r_path = guaranteed_which(session, installer, "R").unwrap();
        session
            .command(vec![r_path.to_str().unwrap(), "CMD", "build", "."])
            .run_detecting_problems()?;
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        let r_path = guaranteed_which(session, installer, "R").unwrap();
        if session.exists(Path::new("run_tests.sh")) {
            session
                .command(vec!["./run_tests.sh"])
                .run_detecting_problems()?;
        } else if session.exists(Path::new("tests/testthat")) {
            session
                .command(vec![
                    r_path.to_str().unwrap(),
                    "-e",
                    "testthat::test_dir('tests')",
                ])
                .run_detecting_problems()?;
        }
        Ok(())
    }

    fn build(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        // Nothing to do here
        Ok(())
    }

    fn clean(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        Err(crate::buildsystem::Error::Unimplemented)
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        let r_path = guaranteed_which(session, installer, "R").unwrap();
        let mut args = vec![
            r_path.to_str().unwrap().to_string(),
            "CMD".to_string(),
            "INSTALL".to_string(),
            ".".to_string(),
        ];
        if let Some(prefix) = &install_target.prefix.as_ref() {
            args.push(format!("--prefix={}", prefix.to_str().unwrap()));
        }
        session
            .command(args.iter().map(|s| s.as_str()).collect())
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
        let mut ret: Vec<(DependencyCategory, Box<dyn Dependency>)> = vec![];
        let f = std::fs::File::open(self.path.join("DESCRIPTION")).unwrap();
        let description = read_description(f).unwrap();
        for s in description.suggests().unwrap_or_default() {
            ret.push((
                DependencyCategory::Build, /* TODO */
                Box::new(RPackageDependency::from_str(&s)),
            ));
        }
        for s in description.depends().unwrap_or_default() {
            ret.push((
                DependencyCategory::Build,
                Box::new(RPackageDependency::from_str(&s)),
            ));
        }
        for s in description.imports().unwrap_or_default() {
            ret.push((
                DependencyCategory::Build,
                Box::new(RPackageDependency::from_str(&s)),
            ));
        }
        for s in description.linking_to().unwrap_or_default() {
            ret.push((
                DependencyCategory::Build,
                Box::new(RPackageDependency::from_str(&s)),
            ));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        let mut ret = vec![];
        let f = std::fs::File::open(self.path.join("DESCRIPTION")).unwrap();
        let description = read_description(f).unwrap();
        if let Some(package) = description.package() {
            ret.push(Box::new(RPackageOutput::new(&package)) as Box<dyn crate::output::Output>);
        }
        Ok(ret)
    }
}

fn read_description<R: std::io::Read>(
    mut r: R,
) -> Result<r_description::lossless::RDescription, r_description::lossless::Error> {
    // See https://r-pkgs.org/description.html
    let mut s = String::new();
    r.read_to_string(&mut s).unwrap();
    let p: r_description::lossless::RDescription = s.parse().unwrap();
    Ok(p)
}
