use crate::buildsystem::{BuildSystem, Error};
use crate::dependencies::go::{GoDependency, GoPackageDependency};
use std::path::{Path, PathBuf};

#[derive(Debug)]
/// Golang (Go) build system representation.
pub struct Golang {
    path: PathBuf,
}

impl Golang {
    /// Create a new Golang build system instance.
    ///
    /// # Arguments
    /// * `path` - Path to the Go project directory
    ///
    /// # Returns
    /// A new Golang instance
    pub fn new(path: &Path) -> Self {
        Self {
            path: path.to_path_buf(),
        }
    }

    /// Probe a directory to check if it contains a Go project.
    ///
    /// Checks for go.mod, go.sum, or .go files in subdirectories.
    ///
    /// # Arguments
    /// * `path` - Path to check for Go project files
    ///
    /// # Returns
    /// Some(BuildSystem) if a Go project is found, None otherwise
    pub fn probe(path: &Path) -> Option<Box<dyn BuildSystem>> {
        if path.join("go.mod").exists() {
            return Some(Box::new(Self::new(path)));
        }
        if path.join("go.sum").exists() {
            return Some(Box::new(Self::new(path)));
        }
        for entry in path.read_dir().unwrap() {
            let entry = entry.unwrap();
            if !entry.file_type().unwrap().is_dir() {
                continue;
            }
            match entry.path().read_dir() {
                Ok(d) => {
                    for subentry in d {
                        let subentry = subentry.unwrap();
                        if subentry.file_type().unwrap().is_file()
                            && subentry.path().extension() == Some(std::ffi::OsStr::new("go"))
                        {
                            return Some(Box::new(Self::new(path)));
                        }
                    }
                }
                Err(e) if e.kind() == std::io::ErrorKind::PermissionDenied => {
                    // Ignore permission denied errors.
                    log::debug!("Permission denied reading {:?}", entry.path());
                }
                Err(e) => {
                    panic!("Error reading {:?}: {:?}", entry.path(), e);
                }
            }
        }
        None
    }
}

impl BuildSystem for Golang {
    fn name(&self) -> &str {
        "golang"
    }

    fn dist(
        &self,
        _session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _target_directory: &Path,
        _quiet: bool,
    ) -> Result<std::ffi::OsString, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn test(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["go", "test", "./..."])
            .run_detecting_problems()?;
        Ok(())
    }

    fn build(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["go", "build"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn clean(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
    ) -> Result<(), crate::buildsystem::Error> {
        session.command(vec!["go", "clean"]).check_call()?;
        Ok(())
    }

    fn install(
        &self,
        session: &dyn crate::session::Session,
        _installer: &dyn crate::installer::Installer,
        _install_target: &crate::buildsystem::InstallTarget,
    ) -> Result<(), crate::buildsystem::Error> {
        session
            .command(vec!["go", "install"])
            .run_detecting_problems()?;
        Ok(())
    }

    fn get_declared_dependencies(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<
        Vec<(
            crate::buildsystem::DependencyCategory,
            Box<dyn crate::dependency::Dependency>,
        )>,
        crate::buildsystem::Error,
    > {
        let mut ret = vec![];
        let go_mod_path = self.path.join("go.mod");
        if go_mod_path.exists() {
            let f = std::fs::File::open(go_mod_path).unwrap();
            ret.extend(
                go_mod_dependencies(f)
                    .into_iter()
                    .map(|dep| (crate::buildsystem::DependencyCategory::Build, dep)),
            );
        }
        Ok(ret)
    }

    fn get_declared_outputs(
        &self,
        _session: &dyn crate::session::Session,
        _fixers: Option<&[&dyn crate::fix_build::BuildFixer<crate::installer::Error>]>,
    ) -> Result<Vec<Box<dyn crate::output::Output>>, crate::buildsystem::Error> {
        Err(Error::Unimplemented)
    }

    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

#[derive(Debug, PartialEq, Eq)]
enum GoModEntry {
    Go(String),
    Require(String, String),
    Exclude(String, String),
    Replace(String, Option<String>, String, Option<String>),
    Retract(String, String),
    Toolchain(String),
    Module(String),
}

impl GoModEntry {
    fn parse(name: &str, args: &[&str]) -> Self {
        match name {
            "go" => GoModEntry::Go(args[0].to_string()),
            "require" => GoModEntry::Require(args[0].to_string(), args[1].to_string()),
            "exclude" => GoModEntry::Exclude(args[0].to_string(), args[1].to_string()),
            "replace" => {
                // replace old [v1] => new [v2]; the version on either side is
                // optional, so locate the "=>" arrow rather than assuming a
                // fixed position.
                let arrow = args
                    .iter()
                    .position(|&a| a == "=>")
                    .expect("replace directive without =>");
                let (lhs, rhs) = args.split_at(arrow);
                let rhs = &rhs[1..];
                let old = lhs[0].to_string();
                let old_version = lhs.get(1).map(|s| s.to_string());
                let new = rhs[0].to_string();
                let new_version = rhs.get(1).map(|s| s.to_string());
                GoModEntry::Replace(old, old_version, new, new_version)
            }
            "retract" => GoModEntry::Retract(args[0].to_string(), args[1].to_string()),
            "toolchain" => GoModEntry::Toolchain(args[0].to_string()),
            "module" => GoModEntry::Module(args[0].to_string()),
            _ => panic!("unknown go.mod directive: {}", name),
        }
    }
}

fn parse_go_mod<R: std::io::Read>(f: R) -> Vec<GoModEntry> {
    let f = std::io::BufReader::new(f);
    let mut ret = vec![];
    use std::io::BufRead;
    let lines = f
        .lines()
        .map(|l| l.unwrap())
        .filter(|l| !l.ends_with("// indirect"))
        .map(|l| l.split("//").next().unwrap().to_string())
        .collect::<Vec<_>>();
    let mut line_iter = lines.iter();
    while let Some(mut line) = line_iter.next() {
        let parts = line.trim().split(" ").collect::<Vec<_>>();
        if parts.is_empty() || parts == [""] {
            continue;
        }
        if parts.len() == 2 && parts[1] == "(" {
            line = line_iter.next().unwrap();
            while line.trim() != ")" {
                ret.push(GoModEntry::parse(
                    parts[0],
                    line.trim().split(' ').collect::<Vec<_>>().as_slice(),
                ));
                line = line_iter.next().expect("unexpected EOF");
            }
        } else {
            ret.push(GoModEntry::parse(parts[0], &parts[1..]));
        }
    }
    ret
}

fn go_mod_dependencies<R: std::io::Read>(r: R) -> Vec<Box<dyn crate::dependency::Dependency>> {
    let mut ret: Vec<Box<dyn crate::dependency::Dependency>> = vec![];
    for entry in parse_go_mod(r) {
        match entry {
            GoModEntry::Go(version) => {
                ret.push(Box::new(GoDependency::new(Some(&version))));
            }
            GoModEntry::Require(name, version) => {
                ret.push(Box::new(GoPackageDependency::new(
                    &name,
                    Some(version.strip_prefix('v').unwrap()),
                )));
            }
            GoModEntry::Exclude(_name, _version) => {
                // TODO(jelmer): Create conflicts?
            }
            GoModEntry::Module(_name) => {}
            GoModEntry::Retract(_name, _version) => {}
            GoModEntry::Toolchain(_name) => {}
            GoModEntry::Replace(_old, _old_version, _new, _new_version) => {
                // TODO(jelmer): do.. something?
            }
        }
    }
    ret
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_replace_no_versions() {
        let entries = parse_go_mod(
            "replace github.com/scip-code/scip/bindings/go/scip => ./bindings/go/scip\n".as_bytes(),
        );
        assert_eq!(
            entries,
            vec![GoModEntry::Replace(
                "github.com/scip-code/scip/bindings/go/scip".to_string(),
                None,
                "./bindings/go/scip".to_string(),
                None,
            )]
        );
    }

    #[test]
    fn test_replace_with_versions() {
        let entries =
            parse_go_mod("replace example.com/old v1.2.3 => example.com/new v4.5.6\n".as_bytes());
        assert_eq!(
            entries,
            vec![GoModEntry::Replace(
                "example.com/old".to_string(),
                Some("v1.2.3".to_string()),
                "example.com/new".to_string(),
                Some("v4.5.6".to_string()),
            )]
        );
    }

    #[test]
    fn test_replace_new_version_only() {
        let entries =
            parse_go_mod("replace example.com/old => example.com/new v4.5.6\n".as_bytes());
        assert_eq!(
            entries,
            vec![GoModEntry::Replace(
                "example.com/old".to_string(),
                None,
                "example.com/new".to_string(),
                Some("v4.5.6".to_string()),
            )]
        );
    }

    #[test]
    fn test_require_and_go() {
        let entries = parse_go_mod("go 1.21\n\nrequire example.com/dep v1.0.0\n".as_bytes());
        assert_eq!(
            entries,
            vec![
                GoModEntry::Go("1.21".to_string()),
                GoModEntry::Require("example.com/dep".to_string(), "v1.0.0".to_string()),
            ]
        );
    }
}
