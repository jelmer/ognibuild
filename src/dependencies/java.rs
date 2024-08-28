use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JavaClassDependency {
    classname: String,
}

impl JavaClassDependency {
    pub fn new(classname: &str) -> Self {
        Self {
            classname: classname.to_string(),
        }
    }
}

impl Dependency for JavaClassDependency {
    fn family(&self) -> &'static str {
        "java-class"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::dependencies::debian::IntoDebianDependency for JavaClassDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        apt.satisfy(vec!["java-propose-classpath"]).unwrap();
        let output = String::from_utf8(apt.session.command(vec!["java-propose-classpath", &format!("-c{}", &self.classname)]).check_output().unwrap()).unwrap();
        let classpath = output.trim_matches(':').trim().split(':').collect::<Vec<&str>>();
        if classpath.is_empty() {
            None
        } else {
            Some(classpath.iter().map(|path| crate::dependencies::debian::DebianDependency::new(path)).collect())
        }
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJavaClass {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JavaClassDependency::new(&self.classname)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JDKDependency;

impl Dependency for JDKDependency {
    fn family(&self) -> &'static str {
        "jdk"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["javac", "-version"])
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

impl crate::dependencies::debian::IntoDebianDependency for JDKDependency {
    fn try_into_debian_dependency(&self, _apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new("default-jdk")])
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJDK {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JDKDependency))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JREDependency;

impl Dependency for JREDependency {
    fn family(&self) -> &'static str {
        "jre"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["java", "-version"])
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

impl crate::dependencies::debian::IntoDebianDependency for JREDependency {
    fn try_into_debian_dependency(&self, _apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        Some(vec![crate::dependencies::debian::DebianDependency::new("default-jre")])
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJRE {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JREDependency))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JDKFileDependency {
    jdk_path: std::path::PathBuf,
    filename: String,
}

impl JDKFileDependency {
    pub fn new(jdk_path: &str, filename: &str) -> Self {
        Self {
            jdk_path: std::path::PathBuf::from(jdk_path.to_string()),
            filename: filename.to_string(),
        }
    }

    pub fn path(&self) -> std::path::PathBuf {
        self.jdk_path.join(&self.filename)
    }
}

impl Dependency for JDKFileDependency {
    fn family(&self) -> &'static str {
        "jdk-file"
    }

    fn present(&self, _session: &dyn Session) -> bool {
        self.path().exists()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        false
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

impl crate::dependencies::debian::IntoDebianDependency for JDKFileDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let path =regex::escape(self.jdk_path.to_str().unwrap()) + ".*/" + &regex::escape(self.filename.as_str());
        let names = apt.get_packages_for_paths(vec![path.as_str()], true, false).unwrap();

        if names.is_empty() {
            None
        } else {
            Some(names.iter().map(|name| crate::dependencies::debian::DebianDependency::simple(name)).collect())
        }
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJDKFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JDKFileDependency::new(&self.jdk_path, &self.filename)))
    }
}
