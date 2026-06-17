//! Dependency resolution using dnf on RPM-based systems.
//!
//! This resolver converts generic dependencies into RPM package names by asking
//! dnf which package provides a given file or capability (via `repoquery`), and
//! then installs them using `dnf`.
//!
//! Only dnf is targeted: on modern RHEL/Fedora `yum` is just a compatibility
//! wrapper around dnf, and the `repoquery` syntax used here is dnf-specific.
//! Genuine legacy yum (CentOS 7 and earlier) is end-of-life and not supported.

use crate::dependencies::{
    BinaryDependency, CHeaderDependency, LibraryDependency, PathDependency, PkgConfigDependency,
};
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;

/// Find the dnf binary available in the session.
///
/// Prefers `dnf5` (Fedora 41+) over the older `dnf` when both are present.
fn dnf_command(session: &dyn Session) -> Option<&'static str> {
    ["dnf5", "dnf"]
        .into_iter()
        .find(|cmd| crate::session::which(session, cmd).is_some())
}

/// Run `repoquery` with the given selector and reduce the resulting NEVRA list
/// to a sorted, deduplicated list of package names.
///
/// `repoquery` queries the configured repositories, so it finds packages even
/// when they are not installed, and does not require root.
fn repoquery(session: &dyn Session, dnf: &str, args: &[&str]) -> Vec<String> {
    let mut argv = vec![dnf, "repoquery", "--quiet"];
    argv.extend_from_slice(args);
    let output = match session
        .command(argv.clone())
        .cwd(std::path::Path::new("/"))
        .check_output()
    {
        Ok(output) => output,
        Err(e) => {
            log::debug!("{:?} failed: {}", argv, e);
            return vec![];
        }
    };

    let mut names = String::from_utf8_lossy(&output)
        .lines()
        .map(|line| line.trim())
        .filter(|line| !line.is_empty())
        // repoquery prints full NEVRA strings (name-epoch:version-release.arch);
        // reduce them to the bare package name.
        .map(nevra_to_name)
        .collect::<Vec<_>>();
    names.sort();
    names.dedup();
    names
}

/// Find packages owning a file at the given path (which may contain globs).
fn packages_for_file(session: &dyn Session, dnf: &str, path: &str) -> Vec<String> {
    repoquery(session, dnf, &["--file", path])
}

/// Find packages providing the given RPM capability (e.g. `pkgconfig(foo)` or a
/// library soname).
fn packages_for_capability(session: &dyn Session, dnf: &str, capability: &str) -> Vec<String> {
    repoquery(session, dnf, &["--whatprovides", capability])
}

/// Reduce an RPM NEVRA string (e.g. `zlib-devel-1.2.11-34.el9.x86_64`) to the
/// bare package name (`zlib-devel`).
fn nevra_to_name(nevra: &str) -> String {
    // A NEVRA is name-[epoch:]version-release.arch. Strip the trailing
    // version-release.arch by removing the last two hyphen-separated fields.
    let parts: Vec<&str> = nevra.rsplitn(3, '-').collect();
    if parts.len() == 3 {
        parts[2].to_string()
    } else {
        nevra.to_string()
    }
}

/// Convert a generic dependency into candidate RPM package names.
fn dependency_to_dnf_packages(
    session: &dyn Session,
    dnf: &str,
    dep: &dyn Dependency,
) -> Option<Vec<String>> {
    if let Some(dep) = dep.as_any().downcast_ref::<BinaryDependency>() {
        let mut names = packages_for_file(session, dnf, &format!("/usr/bin/{}", dep.binary_name()));
        if names.is_empty() {
            names = packages_for_file(session, dnf, &format!("/usr/sbin/{}", dep.binary_name()));
        }
        return Some(names);
    }

    if let Some(dep) = dep.as_any().downcast_ref::<PkgConfigDependency>() {
        let mut names =
            packages_for_capability(session, dnf, &format!("pkgconfig({})", dep.module()));
        if names.is_empty() {
            names = packages_for_file(session, dnf, &format!("*/pkgconfig/{}.pc", dep.module()));
        }
        return Some(names);
    }

    if let Some(dep) = dep.as_any().downcast_ref::<CHeaderDependency>() {
        return Some(packages_for_file(
            session,
            dnf,
            &format!("*/include/{}", dep.header()),
        ));
    }

    if let Some(dep) = dep.as_any().downcast_ref::<LibraryDependency>() {
        // Prefer the soname capability, falling back to the on-disk file.
        let mut names = packages_for_capability(session, dnf, &format!("lib{}.so", dep.library()));
        if names.is_empty() {
            names = packages_for_file(session, dnf, &format!("*/lib{}.so*", dep.library()));
        }
        return Some(names);
    }

    if let Some(dep) = dep.as_any().downcast_ref::<PathDependency>() {
        if let Some(path) = dep.path().to_str() {
            if path.starts_with('/') {
                return Some(packages_for_file(session, dnf, path));
            }
        }
        return None;
    }

    None
}

/// Installer that resolves and installs dependencies using dnf.
pub struct DnfResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> DnfResolver<'a> {
    /// Create a new dnf resolver for the given session.
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    /// Resolve a dependency to RPM package names, returning the dnf binary
    /// alongside the packages.
    fn resolve(&self, dep: &dyn Dependency) -> Result<(&'static str, Vec<String>), Error> {
        let dnf = dnf_command(self.session).ok_or(Error::UnknownDependencyFamily)?;
        match dependency_to_dnf_packages(self.session, dnf, dep) {
            Some(packages) if !packages.is_empty() => Ok((dnf, packages)),
            _ => Err(Error::UnknownDependencyFamily),
        }
    }
}

impl<'a> Installer for DnfResolver<'a> {
    fn install(&self, dep: &dyn Dependency, scope: InstallationScope) -> Result<(), Error> {
        if scope != InstallationScope::Global {
            return Err(Error::UnsupportedScope(scope));
        }

        let (dnf, packages) = self.resolve(dep)?;

        let mut args = vec![dnf, "install", "-y"];
        args.extend(packages.iter().map(|s| s.as_str()));
        log::info!("dnf: running {:?}", args);

        self.session
            .command(args)
            .cwd(std::path::Path::new("/"))
            .user("root")
            .run_detecting_problems()?;
        Ok(())
    }

    fn explain(
        &self,
        dep: &dyn Dependency,
        scope: InstallationScope,
    ) -> Result<Explanation, Error> {
        if scope != InstallationScope::Global {
            return Err(Error::UnsupportedScope(scope));
        }

        let (dnf, packages) = self.resolve(dep)?;

        let mut command = vec![dnf.to_string(), "install".to_string(), "-y".to_string()];
        command.extend(packages.iter().cloned());
        Ok(Explanation {
            message: format!("Install {}", packages.join(", ")),
            command: Some(command),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_nevra_to_name() {
        assert_eq!(
            nevra_to_name("zlib-devel-1.2.11-34.el9.x86_64"),
            "zlib-devel"
        );
        assert_eq!(nevra_to_name("bash-5.1.8-6.el9.x86_64"), "bash");
        assert_eq!(
            nevra_to_name("python3-devel-3.9.18-1.el9.x86_64"),
            "python3-devel"
        );
        // An epoch is glued to the version, so the name is still recovered.
        assert_eq!(nevra_to_name("foo-2:1.0-3.el9.x86_64"), "foo");
        // Already a bare name (no version-release suffix).
        assert_eq!(nevra_to_name("foo"), "foo");
    }
}
