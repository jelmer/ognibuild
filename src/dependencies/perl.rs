use crate::dependency::{Dependency};
use crate::installer::{Explanation, Error, InstallationScope, Installer};
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::path::Path;

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

fn known_predeclared_module(name: &str) -> Option<&str> {
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

impl PerlPreDeclaredDependency {
    pub fn new(name: &str) -> Self {
        Self {
            name: name.to_string(),
        }
    }
}

impl Dependency for PerlPreDeclaredDependency {
    fn family(&self) -> &'static str {
        "perl-predeclared"
    }

    fn present(&self, session: &dyn Session) -> bool {
        if let Some(module) = known_predeclared_module(&self.name) {
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

impl crate::dependencies::debian::IntoDebianDependency for PerlPreDeclaredDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        if let Some(module) = known_predeclared_module(&self.name) {
            PerlModuleDependency::simple(module).try_into_debian_dependency(apt)
        } else {
            None
        }
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPerlPredeclared {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        match known_predeclared_module(self.0.as_str()) {
            Some(_module) => Some(Box::new(PerlModuleDependency::simple(self.0.as_str()))),
            None => {
                log::warn!("Unknown predeclared function: {}", self.0);
                None
            }
        }
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

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPerlFile {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PerlFileDependency {
            filename: self.filename.clone(),
        }))
    }
}

pub struct CPAN<'a> {
    session: &'a dyn Session,
    skip_tests: bool,
}

impl<'a> CPAN<'a> {
    pub fn new(session: &'a dyn Session, skip_tests: bool) -> Self {
        Self {
            session,
            skip_tests,
        }
    }

    fn cmd(&self, reqs: &[&PerlModuleDependency], _scope: InstallationScope) -> Result<Vec<String>, Error> {
        let mut ret = vec!["cpan".to_string(), "-i".to_string()];
        if self.skip_tests {
            ret.push("-T".to_string());
        }
        ret.extend(reqs.iter().map(|req| req.module.clone()));
        Ok(ret)
    }
}

impl<'a> Installer for CPAN<'a> {

    fn explain(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<Explanation, Error> {
        if let Some(dep) = dep.as_any().downcast_ref::<PerlModuleDependency>() {
            let cmd = self.cmd(&[&dep], scope)?;
            let explanation = Explanation {
                message: "Install the following Perl modules".to_string(),
                command: Some(cmd),
            };
            Ok(explanation)
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }

    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        let env = maplit::hashmap! {
            "PERL_MM_USE_DEFAULT".to_string() => "1".to_string(),
            "PERL_MM_OPT".to_string() => "".to_string(),
            "PERL_MB_OPT".to_string() => "".to_string(),
        };

        let user = match scope {
            InstallationScope::User => None,
            InstallationScope::Global => Some("root"),
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        };

        let dep = dep.as_any().downcast_ref::<PerlModuleDependency>().ok_or(Error::UnknownDependencyFamily)?;
        let cmd = self.cmd(&[dep], scope)?;
        log::info!("CPAN: running {:?}", cmd);

        let mut cmd = self.session.command(
            cmd.iter().map(|s| s.as_str()).collect()).env(env);

        if let Some(user) = user {
            cmd = cmd.user(user);
        }

        cmd.run_detecting_problems()?;

        Ok(())
    }

    fn explain_some(
        &self,
        deps: Vec<Box<dyn Dependency>>,
        scope: InstallationScope,
    ) -> Result<(Vec<Explanation>, Vec<Box<dyn Dependency>>), Error> {
        let mut explanations = Vec::new();
        let mut failed = Vec::new();
        for dep in deps {
            match self.explain(&*dep, scope) {
                Ok(explanation) => explanations.push(explanation),
                Err(Error::UnknownDependencyFamily) => failed.push(dep),
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok((explanations, failed))
    }

    fn install_some(
        &self,
        deps: Vec<Box<dyn Dependency>>,
        scope: InstallationScope,
    ) -> Result<(Vec<Box<dyn Dependency>>, Vec<Box<dyn Dependency>>), Error> {
        let mut installed = Vec::new();
        let mut failed = Vec::new();

        for dep in deps {
            match self.install(&*dep, scope) {
                Ok(()) => installed.push(dep),
                Err(Error::UnknownDependencyFamily) => failed.push(dep),
                Err(e) => {
                    return Err(e);
                }
            }
        }
        Ok((installed, failed))
    }
}

pub const DEFAULT_PERL_PATHS: &[&str] = &[
        "/usr/share/perl5",
        "/usr/lib/.*/perl5/.*",
        "/usr/lib/.*/perl-base",
        "/usr/lib/.*/perl/[^/]+",
        "/usr/share/perl/[^/]+",
    ];

impl crate::dependencies::debian::IntoDebianDependency for PerlModuleDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let (regex, paths) = if let (Some(inc), Some(filename)) = (self.inc.as_ref(), self.filename.as_ref()) {
            (false, inc.iter().map(|s| Path::new(s).join(filename)).collect())
        } else if let Some(filename) = &self.filename {
            if !Path::new(filename).is_absolute() {
                (true, DEFAULT_PERL_PATHS.iter().map(|s| Path::new(s).join(filename)).collect())
            } else {
                (false, vec![Path::new(filename).to_path_buf()])
            }
        } else {
            (true, DEFAULT_PERL_PATHS.iter().map(|s| Path::new(s).join(format!("{}.pm", &self.module.replace("::", "/")))).collect())
        };

        let packages = apt.get_packages_for_paths(paths.iter().map(|s| s.to_str().unwrap()).collect::<Vec<_>>(), regex, false).unwrap();

        Some(packages.into_iter().map(|p| crate::dependencies::debian::DebianDependency::simple(&p)).collect())
    }
}

impl crate::dependencies::debian::IntoDebianDependency for PerlFileDependency {
    fn try_into_debian_dependency(&self, apt: &crate::debian::apt::AptManager) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let packages = apt.get_packages_for_paths(vec![&self.filename], false, false).unwrap();

        Some(packages.into_iter().map(|p| crate::dependencies::debian::DebianDependency::simple(&p)).collect())
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPerlModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PerlModuleDependency {
            module: self.module.clone(),
            filename: self.filename.clone(),
            inc: self.inc.clone(),
        }))
    }
}
