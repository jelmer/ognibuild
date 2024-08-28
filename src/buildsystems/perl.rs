use std::collections::HashMap;
use std::io::Read;
use crate::buildsystem::{guaranteed_which, BuildSystem};
use crate::session::Session;
use crate::fix_build::BuildFixer;
use crate::installer::Error as InstallerError;
use crate::dependencies::perl::PerlModuleDependency;
use crate::buildsystem::DependencyCategory;
use std::path::{Path,PathBuf};

fn read_cpanfile(session: &dyn Session, args: Vec<&str>, category: DependencyCategory, fixers: &[&dyn BuildFixer<InstallerError>]) -> impl Iterator<Item = (DependencyCategory, PerlModuleDependency)> {
    let mut argv = vec!["cpanfile-dump"];
    argv.extend(args);

    session.command(argv).run_fixing_problems(fixers).unwrap().into_iter().filter_map(move |line| {
        let line = line.trim();
        if !line.is_empty() {
            Some((category.clone(), PerlModuleDependency::simple(line)))
        } else {
            None
        }
    })
}


pub fn declared_deps_from_cpanfile(session: &dyn Session, fixers: &[&dyn BuildFixer<InstallerError>]) -> Vec<(DependencyCategory, PerlModuleDependency)> {
    read_cpanfile(session, vec!["--configure", "--build"], DependencyCategory::Build, fixers).chain(
        read_cpanfile(session, vec!["--test"], DependencyCategory::Test, fixers)
    ).collect()
}

#[derive(Debug, Clone, PartialEq, Eq, serde::Deserialize)]
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


pub fn declared_deps_from_meta_yml<R: Read>(f: R) -> Vec<(DependencyCategory, PerlModuleDependency)> {
    // See http://module-build.sourceforge.net/META-spec-v1.4.html for the specification of the format.
    let data: Meta = serde_yaml::from_reader(f).unwrap();

    let mut ret = vec![];

    // TODO: handle versions
    for (name, _version) in &data.requires {
        ret.push((DependencyCategory::Universal, PerlModuleDependency::simple(name)));
    }
    for (name, _version) in &data.build_requires {
        ret.push((DependencyCategory::Build, PerlModuleDependency::simple(name)));
    }
    for (name, _version) in &data.configure_requires {
        ret.push((DependencyCategory::Build, PerlModuleDependency::simple(name)));
    }
    // TODO(jelmer): recommends
    ret
}

pub struct DistZilla {
    path: PathBuf,
    dist_inkt_class: Option<String>,
}

impl DistZilla {
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
                dist_inkt_class = Some(value[1..value.len()-1].to_string());
                break;
            }
        }
        Self {
            path,
            dist_inkt_class,
        }
    }

    pub fn setup(&self, installer: &dyn crate::installer::Installer) -> Result<(), crate::installer::Error> {
        let dep = crate::dependencies::perl::PerlModuleDependency::simple("Dist::Inkt");
        installer.install(&dep, crate::installer::InstallationScope::Global)?;
        Ok(())
    }

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
            session.command(vec![guaranteed_which(session, installer, "distinkt-dist").unwrap().to_str().unwrap()]).quiet(quiet).run_detecting_problems()?;
        } else {
            // Default to invoking Dist::Zilla
            session.command(vec![guaranteed_which(session, installer, "dzil").unwrap().to_str().unwrap(), "build", "--tgz"]).quiet(quiet).run_detecting_problems()?;
        }
        Ok(dc.copy_single(target_directory).unwrap().unwrap())
    }

    fn test(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        // see also https://perlmaven.com/how-to-run-the-tests-of-a-typical-perl-module
        self.setup(installer)?;
        session.command(vec![guaranteed_which(session, installer, "dzil").unwrap().to_str().unwrap(), "test"]).run_detecting_problems()?;
        Ok(())
    }

    fn build(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        self.setup(installer)?;
        session.command(vec![guaranteed_which(session, installer, "dzil").unwrap().to_str().unwrap(), "build"]).run_detecting_problems()?;
        Ok(())
    }

    fn clean(&self, session: &dyn Session, installer: &dyn crate::installer::Installer) -> Result<(), crate::buildsystem::Error> {
        todo!()
    }

    fn install(
        &self,
        session: &dyn Session,
        installer: &dyn crate::installer::Installer,
        install_target: &crate::buildsystem::InstallTarget
    ) -> Result<(), crate::buildsystem::Error> {
        todo!()
    }

    fn get_declared_dependencies(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<(DependencyCategory, Box<dyn crate::dependency::Dependency>)>, crate::buildsystem::Error> {
        let mut ret = vec![];
        if self.path.exists() {
            let lines = session.command(vec!["dzil", "authordeps"]).run_fixing_problems(fixers.unwrap_or(&[])).unwrap();
            for entry in lines {
                ret.push((DependencyCategory::Build, Box::new(PerlModuleDependency::simple(entry.trim())) as Box<dyn crate::dependency::Dependency>));
            }
        }
        if self.path.parent().unwrap().join("cpanfile").exists() {
            ret.extend(declared_deps_from_cpanfile(session, fixers.unwrap_or(&[])).into_iter().map(|(category, dep)| (category, Box::new(dep) as Box<dyn crate::dependency::Dependency>)));
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        session: &dyn Session,
        fixers: Option<&[&dyn crate::fix_build::BuildFixer<InstallerError>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        todo!()
    }
}
