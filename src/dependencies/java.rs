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
