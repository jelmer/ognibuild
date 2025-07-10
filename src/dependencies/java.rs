use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency representing a Java class.
pub struct JavaClassDependency {
    /// The name of the Java class
    pub classname: String,
}

impl JavaClassDependency {
    /// Creates a new JavaClassDependency with the given class name.
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJavaClass {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JavaClassDependency::new(&self.classname)))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency representing the Java Development Kit (JDK).
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJDK {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JDKDependency))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency representing the Java Runtime Environment (JRE).
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJRE {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JREDependency))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency representing a specific file in the JDK.
pub struct JDKFileDependency {
    /// The path to the JDK installation
    pub jdk_path: std::path::PathBuf,
    /// The filename within the JDK
    pub filename: String,
}

impl JDKFileDependency {
    /// Creates a new JDKFileDependency with the given JDK path and filename.
    pub fn new(jdk_path: &str, filename: &str) -> Self {
        Self {
            jdk_path: std::path::PathBuf::from(jdk_path.to_string()),
            filename: filename.to_string(),
        }
    }

    /// Returns the full path to the JDK file.
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingJDKFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(JDKFileDependency::new(
            &self.jdk_path,
            &self.filename,
        )))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::buildlog::ToDependency;

    #[test]
    fn test_java_class_dependency_new() {
        let dependency = JavaClassDependency::new("org.apache.commons.lang3.StringUtils");
        assert_eq!(dependency.classname, "org.apache.commons.lang3.StringUtils");
    }

    #[test]
    fn test_java_class_dependency_family() {
        let dependency = JavaClassDependency::new("org.apache.commons.lang3.StringUtils");
        assert_eq!(dependency.family(), "java-class");
    }

    #[test]
    fn test_java_class_dependency_as_any() {
        let dependency = JavaClassDependency::new("org.apache.commons.lang3.StringUtils");
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<JavaClassDependency>().is_some());
    }

    #[test]
    fn test_missing_java_class_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingJavaClass {
            classname: "org.apache.commons.lang3.StringUtils".to_string(),
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "java-class");
        let java_dep = dep.as_any().downcast_ref::<JavaClassDependency>().unwrap();
        assert_eq!(java_dep.classname, "org.apache.commons.lang3.StringUtils");
    }

    #[test]
    fn test_jdk_dependency_family() {
        let dependency = JDKDependency;
        assert_eq!(dependency.family(), "jdk");
    }

    #[test]
    fn test_jdk_dependency_as_any() {
        let dependency = JDKDependency;
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<JDKDependency>().is_some());
    }

    #[test]
    fn test_missing_jdk_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingJDK {
            jdk_path: "/usr/lib/jvm/default-java".to_string(),
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "jdk");
        assert!(dep.as_any().downcast_ref::<JDKDependency>().is_some());
    }

    #[test]
    fn test_jre_dependency_family() {
        let dependency = JREDependency;
        assert_eq!(dependency.family(), "jre");
    }

    #[test]
    fn test_jre_dependency_as_any() {
        let dependency = JREDependency;
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<JREDependency>().is_some());
    }

    #[test]
    fn test_missing_jre_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingJRE;
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "jre");
        assert!(dep.as_any().downcast_ref::<JREDependency>().is_some());
    }

    #[test]
    fn test_jdk_file_dependency_new() {
        let dependency = JDKFileDependency::new("/usr/lib/jvm/default-java", "javac");
        assert_eq!(
            dependency.jdk_path,
            std::path::PathBuf::from("/usr/lib/jvm/default-java")
        );
        assert_eq!(dependency.filename, "javac");
    }

    #[test]
    fn test_jdk_file_dependency_path() {
        let dependency = JDKFileDependency::new("/usr/lib/jvm/default-java", "javac");
        assert_eq!(
            dependency.path(),
            std::path::PathBuf::from("/usr/lib/jvm/default-java/javac")
        );
    }

    #[test]
    fn test_jdk_file_dependency_family() {
        let dependency = JDKFileDependency::new("/usr/lib/jvm/default-java", "javac");
        assert_eq!(dependency.family(), "jdk-file");
    }

    #[test]
    fn test_jdk_file_dependency_as_any() {
        let dependency = JDKFileDependency::new("/usr/lib/jvm/default-java", "javac");
        let any_dep = dependency.as_any();
        assert!(any_dep.downcast_ref::<JDKFileDependency>().is_some());
    }

    #[test]
    fn test_missing_jdk_file_to_dependency() {
        let problem = buildlog_consultant::problems::common::MissingJDKFile {
            jdk_path: "/usr/lib/jvm/default-java".to_string(),
            filename: "javac".to_string(),
        };
        let dependency = problem.to_dependency();
        assert!(dependency.is_some());
        let dep = dependency.unwrap();
        assert_eq!(dep.family(), "jdk-file");
        let jdk_file_dep = dep.as_any().downcast_ref::<JDKFileDependency>().unwrap();
        assert_eq!(
            jdk_file_dep.jdk_path,
            std::path::PathBuf::from("/usr/lib/jvm/default-java")
        );
        assert_eq!(jdk_file_dep.filename, "javac");
    }
}
