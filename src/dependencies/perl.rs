use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerlModuleDependency {
    pub module: String,
    pub filename: Option<String>,
    pub inc: Option<Vec<String>>,
}

impl PerlModuleDependency {
    pub fn new(module: &str, filename: Option<&str>, inc: Option<Vec<&str>>) -> Self {
        Self {
            module: module.to_string(),
            filename: filename.map(|s| s.to_string()),
            inc: inc.map(|v| v.iter().map(|s| s.to_string()).collect()),
        }
    }

    pub fn simple(module: &str) -> Self {
        Self {
            module: module.to_string(),
            filename: None,
            inc: None,
        }
    }
}

impl Dependency for PerlModuleDependency {
    fn family(&self) -> &'static str {
        "perl-module"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let mut cmd = vec!["perl".to_string(), "-M".to_string(), self.module.clone()];
        if let Some(filename) = &self.filename {
            cmd.push(filename.to_string());
        }
        if let Some(inc) = &self.inc {
            cmd.push("-I".to_string());
            cmd.push(inc.join(":"));
        }
        cmd.push("-e".to_string());
        cmd.push("1".to_string());
        session
            .command(cmd.iter().map(|s| s.as_str()).collect())
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .run()
            .unwrap()
            .success()
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerlPreDeclaredDependency {
    name: String,
}

impl PerlPreDeclaredDependency {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }

    fn known_module(&self, name: &str) -> Option<&str> {
        // TODO(jelmer): Can we obtain this information elsewhere?
        match name {
            "auto_set_repository" => Some("Module::Install::Repository"),
            "author_tests" => Some("Module::Install::AuthorTests"),
            "recursive_author_tests" => Some("Module::Install::AuthorTests"),
            "author_requires" => Some("Module::Install::AuthorRequires"),
            "readme_from" => Some("Module::Install::ReadmeFromPod"),
            "catalyst" => Some("Module::Install::Catalyst"),
            "githubmeta" => Some("Module::Install::GithubMeta"),
            "use_ppport" => Some("Module::Install::XSUtil"),
            "pod_from" => Some("Module::Install::PodFromEuclid"),
            "write_doap_changes" => Some("Module::Install::DOAPChangeSets"),
            "use_test_base" => Some("Module::Install::TestBase"),
            "jsonmeta" => Some("Module::Install::JSONMETA"),
            "extra_tests" => Some("Module::Install::ExtraTests"),
            "auto_set_bugtracker" => Some("Module::Install::Bugtracker"),
            _ => None,
        }
    }
}

impl Dependency for PerlPreDeclaredDependency {
    fn family(&self) -> &'static str {
        "perl-predeclared"
    }

    fn present(&self, session: &dyn Session) -> bool {
        if let Some(module) = self.known_module(&self.name) {
            PerlModuleDependency::simple(module).present(session)
        } else {
            todo!()
        }
    }

    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PerlFileDependency {
    filename: String,
}

impl PerlFileDependency {
    pub fn new(filename: &str) -> Self {
        Self {
            filename: filename.to_string(),
        }
    }
}

impl Dependency for PerlFileDependency {
    fn family(&self) -> &'static str {
        "perl-file"
    }

    fn present(&self, session: &dyn Session) -> bool {
        session
            .command(vec!["perl", "-e", &format!("require '{}'", self.filename)])
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
