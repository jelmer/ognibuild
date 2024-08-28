use std::collections::HashMap;
use std::io::Read;
use crate::session::Session;
use crate::fix_build::{BuildFixer};
use crate::installer::Error as InstallerError;
use crate::dependencies::perl::PerlModuleDependency;
use crate::buildsystem::DependencyCategory;

fn read_cpanfile(session: &dyn Session, args: Vec<&str>, category: DependencyCategory, fixers: &[&dyn BuildFixer<InstallerError>]) -> impl Iterator<Item = (DependencyCategory, PerlModuleDependency)> {
    let mut argv = vec!["cpanfile-dump"];
    argv.extend(args);

    session.command(argv).run_fixing_problems(fixers).unwrap().into_iter().filter_map(move |line| {
        let line = line.trim();
        if !line.is_empty() {
            Some((category, PerlModuleDependency::simple(line)))
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
