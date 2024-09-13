use crate::debian::apt::AptManager;
use crate::dependencies::debian::DebianDependency;
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use debian_control::lossless::relations::{Relation, Relations};
use debian_control::relations::VersionConstraint;
use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};

// TODO: use pep508_rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonPackageDependency {
    package: String,
    python_version: Option<String>,
    specs: Vec<(String, String)>,
}

impl From<&pep508_rs::Requirement> for PythonPackageDependency {
    fn from(requirement: &pep508_rs::Requirement) -> Self {
        Self::from_requirement(requirement)
    }
}

impl PythonPackageDependency {
    pub fn from_requirement(requirement: &pep508_rs::Requirement) -> Self {
        // TODO: properly parse the requirement
        Self {
            package: requirement.name.to_string(),
            python_version: None,
            specs: Vec::new(),
        }
    }

    pub fn from_requirement_str(requirement: &str) -> Self {
        let mut parts = requirement.split(' ');
        let package = parts.next().unwrap();
        let specs = parts
            .map(|part| {
                let mut parts = part.splitn(2, |c: char| c == '=' || c == '<' || c == '>');
                let op = parts.next().unwrap();
                let version = parts.next().unwrap();
                (op.to_string(), version.to_string())
            })
            .collect();
        Self {
            package: package.to_string(),
            python_version: None,
            specs,
        }
    }

    pub fn new(package: &str, python_version: Option<&str>, specs: Vec<(String, String)>) -> Self {
        Self {
            package: package.to_string(),
            python_version: python_version.map(|s| s.to_string()),
            specs,
        }
    }

    pub fn new_with_min_version(package: &str, min_version: &str) -> Self {
        Self {
            package: package.to_string(),
            python_version: None,
            specs: vec![(">=".to_string(), min_version.to_string())],
        }
    }

    pub fn simple(package: &str) -> Self {
        Self {
            package: package.to_string(),
            python_version: None,
            specs: vec![],
        }
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingPythonDistribution
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PythonPackageDependency::new(
            &self.distribution,
            match self.python_version {
                Some(2) => Some("cpython2"),
                Some(3) => Some("cpython3"),
                None => None,
                _ => unimplemented!(),
            },
            self.minimum_version
                .as_ref()
                .map(|x| vec![(">=".to_string(), x.to_string())])
                .unwrap_or_default(),
        )))
    }
}

impl crate::dependencies::debian::FromDebianDependency for PythonPackageDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>> {
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;

        let (_, python_version, name) = lazy_regex::regex_captures!("python([0-9.]*)-(.*)", &name)?;

        let python_version = if python_version.is_empty() {
            None
        } else {
            Some(python_version.to_string())
        };

        Some(Box::new(PythonPackageDependency::new(
            &name,
            python_version.as_deref(),
            min_version
                .map(|x| vec![(">=".to_string(), x.upstream_version)])
                .unwrap_or_default(),
        )))
    }
}

impl Dependency for PythonPackageDependency {
    fn family(&self) -> &'static str {
        "python-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let cmd = match self.python_version.as_deref() {
            Some("cpython3") => "python3",
            Some("cpython2") => "python2",
            Some("pypy") => "pypy",
            Some("pypy3") => "pypy3",
            None => "python3",
            _ => unimplemented!(),
        };
        let text = format!(
            "{}{}",
            self.package,
            self.specs
                .iter()
                .map(|(op, version)| format!("{}{}", op, version))
                .collect::<Vec<String>>()
                .join(",")
        );
        session
            .command(vec![
                cmd,
                "-c",
                &format!(
                    r#"import pkg_resources; pkg_resources.require("""{}""")"#,
                    text
                ),
            ])
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        // TODO: check in the virtualenv, if any
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[cfg(feature = "upstream")]
impl crate::upstream::FindUpstream for PythonPackageDependency {
    fn find_upstream(&self) -> Option<crate::upstream::UpstreamMetadata> {
        upstream_ontologist::providers::python::remote_pypi_metadata(&self.package).ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonModuleDependency {
    module: String,
    minimum_version: Option<String>,
    python_version: Option<String>,
}

impl PythonModuleDependency {
    pub fn new(module: &str, minimum_version: Option<&str>, python_version: Option<&str>) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
            python_version: python_version.map(|s| s.to_string()),
        }
    }

    pub fn simple(module: &str) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: None,
            python_version: None,
        }
    }

    fn python_executable(&self) -> &str {
        match self.python_version.as_deref() {
            Some("cpython3") => "python3",
            Some("cpython2") => "python2",
            Some("pypy") => "pypy",
            Some("pypy3") => "pypy3",
            None => "python3",
            _ => unimplemented!(),
        }
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPythonModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PythonModuleDependency::new(
            &self.module,
            self.minimum_version.as_ref().map(|x| x.as_str()),
            match self.python_version {
                Some(2) => Some("cpython2"),
                Some(3) => Some("cpython3"),
                None => None,
                _ => unimplemented!(),
            },
        )))
    }
}

impl Dependency for PythonModuleDependency {
    fn family(&self) -> &'static str {
        "python-module"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let cmd = [
            self.python_executable().to_string(),
            "-c".to_string(),
            format!(
                r#"import pkgutil; exit(0 if pkgutil.find_loader("{}") else 1)"#,
                self.module
            ),
        ];
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

pub struct PypiResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> PypiResolver<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    pub fn cmd(
        &self,
        reqs: Vec<&PythonPackageDependency>,
        scope: InstallationScope,
    ) -> Result<Vec<String>, Error> {
        let mut cmd = vec!["pip".to_string(), "install".to_string()];
        match scope {
            InstallationScope::User => cmd.push("--user".to_string()),
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        Ok(cmd)
    }
}

impl<'a> Installer for PypiResolver<'a> {
    fn install(&self, requirement: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let args = self.cmd(vec![req], scope)?;
        let mut cmd = self
            .session
            .command(args.iter().map(|x| x.as_str()).collect());

        match scope {
            InstallationScope::Global => {
                cmd = cmd.user("root");
            }
            InstallationScope::User => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }

        cmd.run_detecting_problems()?;
        Ok(())
    }

    fn explain(
        &self,
        requirement: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(vec![req], scope)?;
        Ok(Explanation {
            message: format!("Install pip {}", req.package),
            command: Some(cmd),
        })
    }
}

pub fn python_spec_to_apt_rels(pkg_name: &str, specs: Option<&[(String, String)]>) -> Relations {
    // TODO(jelmer): Dealing with epoch, etc?
    if specs.is_none() {
        return pkg_name.parse().unwrap();
    }

    let mut rels: Vec<Relation> = vec![];
    for (o, v) in specs.unwrap() {
        match o.as_str() {
            "~=" => {
                // PEP 440: For a given release identifier V.N , the compatible
                // release clause is approximately equivalent to the pair of
                // comparison clauses: >= V.N, == V.*
                let mut parts = v.split('.').map(|s| s.to_string()).collect::<Vec<String>>();
                parts.pop();
                let last: isize = parts.pop().unwrap().parse().unwrap();
                parts.push((last + 1).to_string());
                let next_maj_deb_version: debversion::Version = parts.join(".").parse().unwrap();
                let deb_version: debversion::Version = v.parse().unwrap();
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::GreaterThanEqual, deb_version)),
                ));
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::LessThan, next_maj_deb_version)),
                ));
            }
            "!=" => {
                let deb_version: debversion::Version = v.parse().unwrap();
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::GreaterThan, deb_version.clone())),
                ));
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::LessThan, deb_version)),
                ));
            }
            "==" if v.ends_with(".*") => {
                let mut parts = v.split('.').map(|s| s.to_string()).collect::<Vec<String>>();
                parts.pop();
                let last: isize = parts.pop().unwrap().parse().unwrap();
                parts.push((last + 1).to_string());
                let deb_version: debversion::Version = v.parse().unwrap();
                let next_maj_deb_version: debversion::Version = parts.join(".").parse().unwrap();
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::GreaterThanEqual, deb_version)),
                ));
                rels.push(Relation::new(
                    pkg_name,
                    Some((VersionConstraint::LessThan, next_maj_deb_version)),
                ));
            }
            o => {
                let vc = match o {
                    ">=" => VersionConstraint::GreaterThanEqual,
                    "<=" => VersionConstraint::LessThanEqual,
                    "<" => VersionConstraint::LessThan,
                    ">" => VersionConstraint::GreaterThan,
                    "==" => VersionConstraint::Equal,
                    _ => unimplemented!(),
                };
                let v: debversion::Version = v.parse().unwrap();
                rels.push(Relation::new(pkg_name, Some((vc, v))));
            }
        }
    }

    Relations::from(rels.into_iter().map(|r| r.into()).collect::<Vec<_>>())
}

impl crate::dependencies::debian::IntoDebianDependency for PythonPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt_mgr: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        let names = get_package_for_python_package(
            apt_mgr,
            &self.package,
            self.python_version.as_deref(),
            Some(&self.specs),
        );
        Some(names)
    }
}

fn get_package_for_python_package(
    apt_mgr: &AptManager,
    package: &str,
    python_version: Option<&str>,
    specs: Option<&[(String, String)]>,
) -> Vec<DebianDependency> {
    let pypy_regex = format!(
        "/usr/lib/pypy/dist\\-packages/{}-.*\\.(dist|egg)\\-info",
        regex::escape(&package.replace('-', "_"))
    );
    let cpython2_regex = format!(
        "/usr/lib/python2\\.[0-9]/dist\\-packages/{}-.*\\.(dist|egg)\\-info",
        regex::escape(&package.replace('-', "_"))
    );
    let cpython3_regex = format!(
        "/usr/lib/python3/dist\\-packages/{}-.*\\.(dist|egg)\\-info",
        regex::escape(&package.replace('-', "_"))
    );
    let paths = match python_version {
        Some("pypy") => vec![pypy_regex],
        Some("cpython2") => vec![cpython2_regex],
        Some("cpython3") => vec![cpython3_regex],
        None => vec![cpython3_regex, cpython2_regex, pypy_regex],
        _ => unimplemented!(),
    };
    let names = apt_mgr
        .get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, true)
        .unwrap();

    names
        .iter()
        .map(|name| DebianDependency::from(python_spec_to_apt_rels(name, specs)))
        .collect()
}

fn get_possible_python3_paths_for_python_object(mut object_path: &str) -> Vec<PathBuf> {
    let mut cpython3_regexes = vec![];
    loop {
        cpython3_regexes.extend([
            Path::new("/usr/lib/python3/dist-packages")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
            Path::new("/usr/lib/python3/dist-packages").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/lib\\-dynload").join(format!(
                "{}\\.cpython\\-.*\\.so",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python3\\.[0-9]+/")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
        ]);
        object_path = match object_path.rsplit_once('.') {
            Some((o, _)) => o,
            None => break,
        };
    }
    cpython3_regexes
}

fn get_possible_pypy_paths_for_python_object(mut object_path: &str) -> Vec<PathBuf> {
    let mut pypy_regexes = vec![];
    loop {
        pypy_regexes.extend([
            Path::new("/usr/lib/pypy/dist\\-packages")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
            Path::new("/usr/lib/pypy/dist\\-packages").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/pypy/dist\\-packages").join(format!(
                "{}\\.pypy-.*\\.so",
                regex::escape(&object_path.replace('.', "/"))
            )),
        ]);
        object_path = match object_path.rsplit_once('.') {
            Some((o, _)) => o,
            None => break,
        };
    }
    pypy_regexes
}

fn get_possible_python2_paths_for_python_object(mut object_path: &str) -> Vec<PathBuf> {
    let mut cpython2_regexes = vec![];
    loop {
        cpython2_regexes.extend([
            Path::new("/usr/lib/python2\\.[0-9]/dist\\-packages")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
            Path::new("/usr/lib/python2\\.[0-9]/dist\\-packages").join(format!(
                "{}\\.py",
                regex::escape(&object_path.replace('.', "/"))
            )),
            Path::new("/usr/lib/python2.\\.[0-9]/lib\\-dynload").join(format!(
                "{}\\.so",
                regex::escape(&object_path.replace('.', "/"))
            )),
        ]);
        object_path = match object_path.rsplit_once('.') {
            Some((o, _)) => o,
            None => break,
        };
    }
    cpython2_regexes
}

fn get_package_for_python_object_path(
    apt_mgr: &AptManager,
    object_path: &str,
    python_version: Option<&str>,
    specs: &[(String, String)],
) -> Vec<DebianDependency> {
    // Try to find the most specific file
    let paths = match python_version {
        Some("cpython3") => get_possible_python3_paths_for_python_object(object_path),
        Some("cpython2") => get_possible_python2_paths_for_python_object(object_path),
        Some("pypy") => get_possible_pypy_paths_for_python_object(object_path),
        None => get_possible_python3_paths_for_python_object(object_path)
            .into_iter()
            .chain(get_possible_python2_paths_for_python_object(object_path))
            .chain(get_possible_pypy_paths_for_python_object(object_path))
            .collect(),
        _ => unimplemented!(),
    };
    let names = apt_mgr
        .get_packages_for_paths(
            paths.iter().map(|x| x.to_str().unwrap()).collect(),
            true,
            false,
        )
        .unwrap();
    names
        .into_iter()
        .map(|name| DebianDependency::from(python_spec_to_apt_rels(&name, Some(specs))))
        .collect()
}

impl crate::dependencies::debian::IntoDebianDependency for PythonModuleDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        let specs = if let Some(min_version) = &self.minimum_version {
            vec![(">=".to_string(), min_version.to_string())]
        } else {
            vec![]
        };
        Some(get_package_for_python_object_path(
            apt,
            &self.module,
            self.python_version.as_deref(),
            specs.as_slice(),
        ))
    }
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingSetupPyCommand
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        match self.0.as_str() {
            "test" => Some(Box::new(PythonPackageDependency::simple("setuptools"))),
            _ => None,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonDependency {
    pub min_version: Option<String>,
}

impl PythonDependency {
    pub fn new(min_version: Option<&str>) -> Self {
        Self {
            min_version: min_version.map(|s| s.to_string()),
        }
    }

    pub fn simple() -> Self {
        Self { min_version: None }
    }

    fn executable(&self) -> &str {
        match &self.min_version {
            Some(min_version) => {
                if min_version.starts_with("2") {
                    "python"
                } else {
                    "python3"
                }
            }
            None => "python3",
        }
    }
}

impl Dependency for PythonDependency {
    fn family(&self) -> &'static str {
        "python"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let cmd = match self.min_version {
            Some(ref min_version) => vec![
                self.executable().to_string(),
                "-c".to_string(),
                format!(
                    "import sys; sys.exit(0 if sys.version_info >= ({}) else 1)",
                    min_version.replace('.', ", ")
                ),
            ],
            None => vec![
                "python3".to_string(),
                "-c".to_string(),
                "import sys; sys.exit(0 if sys.version_info >= (3, 0) else 1)".to_string(),
            ],
        };
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, session: &dyn Session) -> bool {
        // Check if a virtualenv is present
        session.exists(Path::new("bin/python"))
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl From<&pep440_rs::VersionSpecifiers> for PythonDependency {
    fn from(specs: &pep440_rs::VersionSpecifiers) -> Self {
        for specifier in specs.iter() {
            match specifier.operator() {
                pep440_rs::Operator::GreaterThanEqual => {
                    return Self {
                        min_version: Some(specifier.version().to_string()),
                    }
                }
                _ => {}
            }
        }
        Self { min_version: None }
    }
}

impl crate::dependencies::debian::FromDebianDependency for PythonDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>> {
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;
        if name == "python" || name == "python3" {
            Some(Box::new(PythonDependency {
                min_version: min_version.map(|x| x.upstream_version.clone()),
            }))
        } else {
            None
        }
    }
}

impl crate::dependencies::debian::IntoDebianDependency for PythonDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        let mut deps = vec![];
        if let Some(min_version) = &self.min_version {
            if min_version.starts_with("2") {
                deps.push(
                    crate::dependencies::debian::DebianDependency::new_with_min_version(
                        "python",
                        &min_version.parse::<debversion::Version>().unwrap(),
                    ),
                );
            } else {
                deps.push(
                    crate::dependencies::debian::DebianDependency::new_with_min_version(
                        "python3",
                        &min_version.parse::<debversion::Version>().unwrap(),
                    ),
                );
            }
        } else {
            deps.push(crate::dependencies::debian::DebianDependency::simple(
                "python3",
            ));
        }
        Some(deps)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_paths() {
        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/lib\\-dynload/dulwich.cpython\\-.*\\.so"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("dulwich"),
        );
        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo/foo.cpython\\-.*\\.so"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo.cpython\\-.*\\.so"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("cleo.foo"),
        );
    }
}
