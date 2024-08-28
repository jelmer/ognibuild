use crate::fix_build::{BuildFixer, Error};
use crate::installer::{Error as InstallerError, InstallationScope, Installer};
use crate::session::Session;
use buildlog_consultant::problems::common::{
    MinimumAutoconfTooOld, MissingAutoconfMacro, MissingGitIdentity, MissingGnulibDirectory,
    MissingGoSumEntry, MissingSecretGpgKey,
};
use buildlog_consultant::Problem;
use std::io::{Seek, Write};

pub struct GnulibDirectoryFixer<'a> {
    session: &'a dyn Session,
}

impl std::fmt::Debug for GnulibDirectoryFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GnulibDirectoryFixer").finish()
    }
}

impl std::fmt::Display for GnulibDirectoryFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GnulibDirectoryFixer")
    }
}

impl<'a> GnulibDirectoryFixer<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }
}

impl<'a> BuildFixer<InstallerError> for GnulibDirectoryFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MissingGnulibDirectory>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        self.session
            .command(vec!["./gnulib.sh"])
            .check_call()
            .unwrap();
        Ok(true)
    }
}

pub struct GitIdentityFixer<'a> {
    session: &'a dyn Session,
}

impl std::fmt::Debug for GitIdentityFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("GitIdentityFixer").finish()
    }
}

impl std::fmt::Display for GitIdentityFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "GitIdentityFixer")
    }
}

impl<'a> GitIdentityFixer<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }
}

impl<'a> BuildFixer<InstallerError> for GitIdentityFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MissingGitIdentity>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        for name in ["user.email", "user.name"] {
            let output = std::process::Command::new("git")
                .arg("config")
                .arg("--global")
                .arg(name)
                .output()
                .unwrap();
            let value = String::from_utf8(output.stdout).unwrap();
            self.session
                .command(vec!["git", "config", "--global", name, &value])
                .check_call()
                .unwrap();
        }
        Ok(true)
    }
}

pub struct SecretGpgKeyFixer<'a> {
    session: &'a dyn Session,
}

impl std::fmt::Debug for SecretGpgKeyFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("SecretGpgKeyFixer").finish()
    }
}

impl std::fmt::Display for SecretGpgKeyFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "SecretGpgKey")
    }
}

impl<'a> SecretGpgKeyFixer<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }
}

impl<'a> BuildFixer<InstallerError> for SecretGpgKeyFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MissingSecretGpgKey>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        let mut td = tempfile::tempfile().unwrap();
        let script = br#"""Key-Type: 1
Key-Length: 4096
Subkey-Type: 1
Subkey-Length: 4096
Name-Real: Dummy Key for ognibuild
Name-Email: dummy@example.com
Expire-Date: 0
Passphrase: ""
"""#;
        td.write_all(script).unwrap();
        td.seek(std::io::SeekFrom::Start(0)).unwrap();
        self.session
            .command(vec!["gpg", "--gen-key", "--batch", "/dev/stdin"])
            .stdin(td.into())
            .check_call()
            .unwrap();
        Ok(true)
    }
}

pub struct MinimumAutoconfFixer<'a> {
    session: &'a dyn Session,
}

impl std::fmt::Debug for MinimumAutoconfFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MinimumAutoconfFixer").finish()
    }
}

impl std::fmt::Display for MinimumAutoconfFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MinimumAutoconfFixer")
    }
}

impl<'a> MinimumAutoconfFixer<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }
}

impl<'a> BuildFixer<InstallerError> for MinimumAutoconfFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MinimumAutoconfTooOld>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        let problem = problem
            .as_any()
            .downcast_ref::<MinimumAutoconfTooOld>()
            .unwrap();
        for name in ["configure.ac", "configure.in"] {
            let p = self.session.external_path(std::path::Path::new(name));
            let f = std::fs::File::open(&p).unwrap();
            let buf = std::io::BufReader::new(f);
            use std::io::BufRead;
            let mut lines = buf.lines().map(|l| l.unwrap()).collect::<Vec<_>>();
            let mut found = false;
            for line in lines.iter_mut() {
                let m = lazy_regex::regex_find!(r"AC_PREREQ\((.*)\)", &line);
                if m.is_none() {
                    continue;
                }
                *line = format!("AC_PREREQ({})", problem.0);
                found = true;
            }
            if !found {
                lines.insert(0, format!("AC_PREREQ({})", problem.0));
            }
            std::fs::write(
                self.session.external_path(std::path::Path::new(name)),
                lines.concat(),
            )
            .unwrap();
            return Ok(true);
        }
        Ok(false)
    }
}

pub struct MissingGoSumEntryFixer<'a> {
    session: &'a dyn Session,
}

impl std::fmt::Debug for MissingGoSumEntryFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("MissingGoSumEntryFixer").finish()
    }
}

impl std::fmt::Display for MissingGoSumEntryFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "MissingGoSumEntryFixer")
    }
}

impl<'a> MissingGoSumEntryFixer<'a> {
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }
}

impl<'a> BuildFixer<InstallerError> for MissingGoSumEntryFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MissingGoSumEntry>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        let problem = problem
            .as_any()
            .downcast_ref::<MissingGoSumEntry>()
            .unwrap();
        self.session
            .command(vec!["go", "mod", "download", &problem.package])
            .check_call()
            .unwrap();
        Ok(true)
    }
}

pub struct UnexpandedAutoconfMacroFixer<'a> {
    session: &'a dyn Session,
    installer: &'a dyn Installer,
}

impl std::fmt::Debug for UnexpandedAutoconfMacroFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("UnexpandedAutoconfMacroFixer").finish()
    }
}

impl std::fmt::Display for UnexpandedAutoconfMacroFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "UnexpandedAutoconfMacroFixer")
    }
}

impl<'a> UnexpandedAutoconfMacroFixer<'a> {
    pub fn new(session: &'a dyn Session, installer: &'a dyn Installer) -> Self {
        Self { session, installer }
    }
}

impl<'a> BuildFixer<InstallerError> for UnexpandedAutoconfMacroFixer<'a> {
    fn can_fix(&self, problem: &dyn Problem) -> bool {
        problem
            .as_any()
            .downcast_ref::<MissingAutoconfMacro>()
            .is_some()
    }

    fn fix(&self, problem: &dyn Problem, phase: &[&str]) -> Result<bool, Error<InstallerError>> {
        let problem = problem
            .as_any()
            .downcast_ref::<MissingAutoconfMacro>()
            .unwrap();
        let dep = crate::dependencies::autoconf::AutoconfMacroDependency::new(&problem.r#macro);
        self.installer
            .install(&dep, InstallationScope::Global)
            .unwrap();
        self.session
            .command(vec!["autoconf", "-f"])
            .check_call()
            .unwrap();
        Ok(true)
    }
}

pub struct InstallFixer<'a> {
    installer: &'a dyn crate::installer::Installer,
    scope: crate::installer::InstallationScope,
}

impl std::fmt::Debug for InstallFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("InstallFixer").finish()
    }
}

impl std::fmt::Display for InstallFixer<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "upstream requirement fixer")
    }
}

impl<'a> InstallFixer<'a> {
    pub fn new(
        installer: &'a dyn crate::installer::Installer,
        scope: crate::installer::InstallationScope,
    ) -> Self {
        Self { installer, scope }
    }
}

impl<'a> BuildFixer<crate::installer::Error> for InstallFixer<'a> {
    fn can_fix(&self, error: &dyn Problem) -> bool {
        let req = crate::buildlog::problem_to_dependency(error);
        req.is_some()
    }

    fn fix(
        &self,
        error: &dyn Problem,
        phase: &[&str],
    ) -> Result<bool, Error<crate::installer::Error>> {
        let req = crate::buildlog::problem_to_dependency(error);
        if let Some(req) = req {
            self.installer.install(req.as_ref(), self.scope).unwrap();
            Ok(true)
        } else {
            Ok(false)
        }
    }
}
