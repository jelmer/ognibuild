use crate::dependency::{Dependency, Installer, Error, Explanation};
use crate::session::Session;
use serde::{Deserialize, Serialize};

// TODO: use pep508_rs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PythonPackageDependency {
    package: String,
    python_version: Option<String>,
    specs: Vec<(String, String)>,
}

impl PythonPackageDependency {
    pub fn new(package: &str, python_version: Option<&str>, specs: Vec<(String, String)>) -> Self {
        Self {
            package: package.to_string(),
            python_version: python_version.map(|s| s.to_string()),
            specs,
        }
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


pub struct PypiResolver {
    session: Box<dyn Session>,
    user_local: bool,
}

impl PypiResolver {
    pub fn new(session: Box<dyn Session>, user_local: bool) -> Self {
        Self { session, user_local }
    }

    pub fn cmd(&self, reqs: Vec<&PythonPackageDependency>) -> Vec<String> {
        let mut cmd = vec!["pip".to_string(), "install".to_string()];
        if !self.user_local {
            cmd.push("--user".to_string());
        }
        cmd.extend(reqs.iter().map(|req| req.package.clone()));
        cmd
    }
}

impl Installer for PypiResolver {
    fn install(&self, requirement: &dyn Dependency) -> Result<(), Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(vec![req]);
        crate::analyze::run_detecting_problems(self.session.as_ref(), cmd.iter().map(|x| x.as_str()).collect(), None, false, None,  if !self.user_local { Some("root") } else { None }, None, None, None, None)?;
        Ok(())
    }

    fn explain(&self, requirement: &dyn Dependency) -> Result<Explanation, Error> {
        let req = requirement
            .as_any()
            .downcast_ref::<PythonPackageDependency>()
            .ok_or_else(|| Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(vec![req]);
        Ok(Explanation {
            message: format!("Install pip {}", req.package),
            command: Some(cmd),
        })
    }
}
