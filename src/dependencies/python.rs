//! Support for Python package dependencies.
//!
//! This module provides functionality for working with Python package dependencies,
//! including parsing and resolving PEP 508 package requirements, and integrating
//! with package managers.

#[cfg(feature = "debian")]
use crate::debian::apt::AptManager;
#[cfg(feature = "debian")]
use crate::dependencies::debian::DebianDependency;
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
#[cfg(feature = "debian")]
use debian_control::{
    lossless::relations::{Entry, Relation, Relations},
    relations::VersionConstraint,
};
use pep508_rs::pep440_rs;
use serde::{Deserialize, Serialize};
use std::path::Path;

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a Python package.
///
/// This represents a dependency on a Python package from PyPI or another package
/// repository. It uses PEP 508 requirement syntax for expressing version constraints.
pub struct PythonPackageDependency(pep508_rs::Requirement);

impl From<pep508_rs::Requirement> for PythonPackageDependency {
    fn from(requirement: pep508_rs::Requirement) -> Self {
        Self(requirement)
    }
}

impl TryFrom<PythonPackageDependency> for pep508_rs::Requirement {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: PythonPackageDependency) -> Result<Self, Self::Error> {
        Ok(value.0)
    }
}

impl TryFrom<String> for PythonPackageDependency {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: String) -> Result<Self, Self::Error> {
        Self::try_from(value.as_str())
    }
}

impl TryFrom<&str> for PythonPackageDependency {
    type Error = pep508_rs::Pep508Error;

    fn try_from(value: &str) -> Result<Self, Self::Error> {
        use std::str::FromStr;
        let req = pep508_rs::Requirement::from_str(value)?;

        Ok(PythonPackageDependency(req))
    }
}

impl PythonPackageDependency {
    /// Get the package name.
    ///
    /// # Returns
    /// The name of the Python package
    pub fn package(&self) -> String {
        self.0.name.to_string()
    }

    /// Create a new dependency with a minimum version requirement.
    ///
    /// # Arguments
    /// * `package` - The name of the Python package
    /// * `min_version` - The minimum version required
    ///
    /// # Returns
    /// A new PythonPackageDependency
    pub fn new_with_min_version(package: &str, min_version: &str) -> Self {
        Self(pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(package.to_string()).unwrap(),
            version_or_url: Some(min_version_as_version_or_url(min_version)),
            extras: vec![],
            marker: pep508_rs::MarkerTree::TRUE,
            origin: None,
        })
    }

    /// Create a simple dependency with no version constraints.
    ///
    /// # Arguments
    /// * `package` - The name of the Python package
    ///
    /// # Returns
    /// A new PythonPackageDependency
    pub fn simple(package: &str) -> Self {
        Self(pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(package.to_string()).unwrap(),
            version_or_url: None,
            extras: vec![],
            marker: pep508_rs::MarkerTree::TRUE,
            origin: None,
        })
    }
}

/// Convert a minimum version string to a PEP 508 VersionOrUrl.
///
/// # Arguments
/// * `min_version` - The minimum version string
///
/// # Returns
/// A PEP 508 VersionOrUrl with a >= constraint
fn min_version_as_version_or_url(min_version: &str) -> pep508_rs::VersionOrUrl {
    use std::str::FromStr;
    let version_specifiers = std::iter::once(
        pep440_rs::VersionSpecifier::from_pattern(
            pep440_rs::Operator::GreaterThanEqual,
            pep440_rs::VersionPattern::verbatim(pep440_rs::Version::from_str(min_version).unwrap()),
        )
        .unwrap(),
    )
    .collect();
    pep508_rs::VersionOrUrl::VersionSpecifier(version_specifiers)
}

/// Create a PEP 508 marker for a specific Python major version.
///
/// # Arguments
/// * `major_version` - The Python major version (e.g., 2 or 3)
///
/// # Returns
/// A PEP 508 MarkerTree that requires the specified Python version
fn major_python_version_as_marker(major_version: u32) -> pep508_rs::MarkerTree {
    pep508_rs::MarkerTree::expression(pep508_rs::MarkerExpression::Version {
        key: pep508_rs::MarkerValueVersion::PythonVersion,
        specifier: pep440_rs::VersionSpecifier::equals_star_version(pep440_rs::Version::new([
            major_version as u64,
        ])),
    })
}

impl crate::buildlog::ToDependency
    for buildlog_consultant::problems::common::MissingPythonDistribution
{
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        let version_or_url = self
            .minimum_version
            .as_ref()
            .map(|min_version| min_version_as_version_or_url(min_version));
        let marker = self
            .python_version
            .as_ref()
            .map(|python_major_version| {
                major_python_version_as_marker(*python_major_version as u32)
            })
            .unwrap_or(pep508_rs::MarkerTree::TRUE);

        let requirement = pep508_rs::Requirement {
            name: pep508_rs::PackageName::new(self.distribution.clone()).unwrap(),
            version_or_url,
            extras: vec![],
            marker,
            origin: None,
        };

        Some(Box::new(PythonPackageDependency::from(requirement)))
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::FromDebianDependency for PythonPackageDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>> {
        // TODO: handle other things than min version
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;

        let (_, python_version, name) = lazy_regex::regex_captures!("python([0-9.]*)-(.*)", &name)?;

        let major_python_version = if python_version.is_empty() {
            None
        } else {
            Some(python_version.parse::<u32>().unwrap())
        };

        // If a minimum version is present it must convert; otherwise we cannot
        // faithfully represent the dependency, so give up rather than silently
        // dropping the constraint.
        let version_or_url = match min_version {
            Some(v) => Some(min_version_as_version_or_url(
                &debian_to_pep440_version(&v)?.to_string(),
            )),
            None => None,
        };

        Some(Box::new(PythonPackageDependency::from(
            pep508_rs::Requirement {
                name: pep508_rs::PackageName::new(name.to_owned()).unwrap(),
                version_or_url,
                marker: major_python_version
                    .map(major_python_version_as_marker)
                    .unwrap_or(pep508_rs::MarkerTree::TRUE),
                extras: vec![],
                origin: None,
            },
        )))
    }
}

#[derive(Debug, Clone, Default, Copy, Serialize, Deserialize)]
/// Supported Python implementations and versions.
///
/// This enum represents the different Python implementations and versions
/// that can be used to satisfy Python package dependencies.
pub enum PythonVersion {
    /// CPython 2.x
    CPython2,
    /// CPython 3.x (default)
    #[default]
    CPython3,
    /// PyPy (Python 2 compatible)
    PyPy,
    /// PyPy (Python 3 compatible)
    PyPy3,
}

impl PythonVersion {
    /// Get the executable name for this Python version.
    ///
    /// # Returns
    /// The name of the Python executable
    pub fn executable(&self) -> &'static str {
        match self {
            PythonVersion::CPython2 => "python2",
            PythonVersion::CPython3 => "python3",
            PythonVersion::PyPy => "pypy",
            PythonVersion::PyPy3 => "pypy3",
        }
    }
}

impl Dependency for PythonPackageDependency {
    fn family(&self) -> &'static str {
        "python-package"
    }

    fn present(&self, session: &dyn Session) -> bool {
        let python_version = find_python_version(self.0.marker.to_dnf()).unwrap_or_default();
        let cmd = python_version.executable();
        session
            .command(vec![
                cmd,
                "-c",
                &format!(
                    r#"import pkg_resources; pkg_resources.require("""{}""")"#,
                    self.0
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
        let rt = tokio::runtime::Runtime::new().unwrap();
        rt.block_on(upstream_ontologist::providers::python::remote_pypi_metadata(&self.package()))
            .ok()
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on a Python module.
///
/// This represents a dependency on a specific Python module (importable name)
/// rather than a package. Used for checking if specific imports will work.
pub struct PythonModuleDependency {
    module: String,
    minimum_version: Option<String>,
    python_version: Option<PythonVersion>,
}

impl PythonModuleDependency {
    /// Create a new Python module dependency.
    ///
    /// # Arguments
    /// * `module` - The name of the module to import
    /// * `minimum_version` - Optional minimum version requirement
    /// * `python_version` - Optional specific Python version to use
    ///
    /// # Returns
    /// A new PythonModuleDependency
    pub fn new(
        module: &str,
        minimum_version: Option<&str>,
        python_version: Option<PythonVersion>,
    ) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: minimum_version.map(|s| s.to_string()),
            python_version,
        }
    }

    /// Create a simple Python module dependency with no version constraints.
    ///
    /// # Arguments
    /// * `module` - The name of the module to import
    ///
    /// # Returns
    /// A new PythonModuleDependency with no version constraints
    pub fn simple(module: &str) -> Self {
        Self {
            module: module.to_string(),
            minimum_version: None,
            python_version: None,
        }
    }

    /// Get the Python executable to use for this dependency.
    ///
    /// # Returns
    /// The name of the Python executable
    fn python_executable(&self) -> &str {
        self.python_version.unwrap_or_default().executable()
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingPythonModule {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(PythonModuleDependency::new(
            &self.module,
            self.minimum_version.as_deref(),
            match self.python_version {
                Some(2) => Some(PythonVersion::CPython2),
                Some(3) => Some(PythonVersion::CPython3),
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

/// Resolver for Python packages using pip.
///
/// This resolver installs Python packages from PyPI using pip.
pub struct PypiResolver<'a> {
    session: &'a dyn Session,
}

impl<'a> PypiResolver<'a> {
    /// Create a new PypiResolver with the specified session.
    ///
    /// # Arguments
    /// * `session` - The session to use for executing commands
    pub fn new(session: &'a dyn Session) -> Self {
        Self { session }
    }

    /// Generate the pip command for installing the specified requirements.
    ///
    /// # Arguments
    /// * `reqs` - The Python package dependencies to install
    /// * `scope` - The installation scope (user or global)
    ///
    /// # Returns
    /// The pip command as a vector of strings
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
        cmd.extend(reqs.iter().map(|req| req.package().to_string()));
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
            message: format!("Install pip {}", req.0.name),
            command: Some(cmd),
        })
    }
}

#[cfg(feature = "debian")]
/// Convert a PEP 440 version into a Debian version.
///
/// This mirrors the mapping dh-python's `_translate` applies in PEP 440 mode:
/// the only marker rewritten is the pre-release tag (`a`/`b`/`rc`), which gets a
/// leading `~` so that pre-releases sort before the final release. Dev releases
/// (`.devN`), post-releases (`.postN`) and local versions (`+local`) are kept
/// verbatim, matching dh-python. The PEP 440 epoch (`N!`) maps to the Debian
/// epoch (`N:`); dh-python only sets epochs via separate uscan rules, but `N:`
/// is the sole valid Debian rendering.
///
/// The version is rebuilt from the parsed components rather than by rewriting
/// the version string, so it never corrupts `a`/`b`/`rc` letters that appear
/// inside a local version segment (a known dh-python quirk).
pub fn pep440_to_debian_version(version: &pep440_rs::Version) -> debversion::Version {
    let mut upstream = version
        .release()
        .iter()
        .map(|n| n.to_string())
        .collect::<Vec<_>>()
        .join(".");

    if let Some(pre) = version.pre() {
        upstream.push('~');
        upstream.push_str(&pre.to_string());
    }
    if let Some(post) = version.post() {
        upstream.push_str(&format!(".post{post}"));
    }
    if let Some(dev) = version.dev() {
        upstream.push_str(&format!(".dev{dev}"));
    }
    let local = version.local();
    if !local.is_empty() {
        upstream.push('+');
        upstream.push_str(
            &local
                .iter()
                .map(|s| s.to_string())
                .collect::<Vec<_>>()
                .join("."),
        );
    }

    debversion::Version {
        epoch: debian_epoch(version.epoch()),
        upstream_version: upstream,
        debian_revision: None,
    }
}

#[cfg(feature = "debian")]
/// Convert the upstream portion of a Debian version into a PEP 440 version.
///
/// This inverts [`pep440_to_debian_version`]: a `~` before a pre-release tag is
/// dropped to recover the PEP 440 form. The Debian revision (if any) is ignored,
/// since PEP 440 has no equivalent. Returns `None` if the result is not a valid
/// PEP 440 version.
pub fn debian_to_pep440_version(version: &debversion::Version) -> Option<pep440_rs::Version> {
    use std::str::FromStr;
    let mut s = String::new();
    if let Some(epoch) = version.epoch {
        s.push_str(&format!("{epoch}!"));
    }
    // Only '~' immediately followed by a pre-release tag (a/b/rc) is meaningful;
    // it is dropped to recover the PEP 440 spelling. Any other '~' has no PEP 440
    // equivalent and makes the conversion fail.
    let upstream = &version.upstream_version;
    let mut rest = upstream.as_str();
    while let Some(idx) = rest.find('~') {
        s.push_str(&rest[..idx]);
        let after = &rest[idx + 1..];
        if ["rc", "a", "b"].iter().any(|m| after.starts_with(m)) {
            rest = after;
        } else {
            return None;
        }
    }
    s.push_str(rest);
    pep440_rs::Version::from_str(&s).ok()
}

#[cfg(feature = "debian")]
/// Map a PEP 440 epoch onto a Debian epoch. PEP 440 epoch 0 has no Debian
/// representation (epochs are omitted), so it maps to `None`.
///
/// PEP 440 epochs are `u64`; Debian epochs are `u32`. A value that does not fit
/// is saturated to `u32::MAX` rather than silently wrapping. Such epochs do not
/// occur in practice.
fn debian_epoch(epoch: u64) -> Option<u32> {
    match epoch {
        0 => None,
        e => Some(e.try_into().unwrap_or(u32::MAX)),
    }
}

#[cfg(feature = "debian")]
/// Increment the last run of ASCII digits in `s`, preserving any trailing
/// non-digit text. Used to derive the exclusive upper bound for exact `==`
/// constraints, matching dh-python's `(.*)(\d+)(\D*)$` substitution. If `s`
/// contains no digits it is returned unchanged.
fn bump_last_number(s: &str) -> String {
    let bytes = s.as_bytes();
    let Some(end) = bytes.iter().rposition(|b| b.is_ascii_digit()) else {
        return s.to_string();
    };
    let start = bytes[..end]
        .iter()
        .rposition(|b| !b.is_ascii_digit())
        .map_or(0, |i| i + 1);
    let n: u64 = s[start..=end]
        .parse()
        .expect("ASCII digit run parses as u64");
    format!("{}{}{}", &s[..start], n.saturating_add(1), &s[end + 1..])
}

#[cfg(feature = "debian")]
/// Build a Debian version from an epoch and a release segment slice.
fn release_debian_version(epoch: u64, release: &[u64]) -> debversion::Version {
    debversion::Version {
        epoch: debian_epoch(epoch),
        upstream_version: release
            .iter()
            .map(|n| n.to_string())
            .collect::<Vec<_>>()
            .join("."),
        debian_revision: None,
    }
}

#[cfg(feature = "debian")]
/// Build the Debian version that bumps the last release segment of `release`,
/// keeping the other segments: `[1, 4]` -> `1.5`, `[1, 4, 5]` -> `1.4.6`. Used
/// as the exclusive upper bound for `== V.*` / `!= V.*`.
fn bumped_release(epoch: u64, release: &[u64]) -> debversion::Version {
    let mut bumped = release.to_vec();
    match bumped.last_mut() {
        Some(last) => *last = last.saturating_add(1),
        None => bumped.push(1),
    }
    release_debian_version(epoch, &bumped)
}

#[cfg(feature = "debian")]
/// Return the maximum compatible release for a `~=` upper bound, as a Debian
/// version.
///
/// This mirrors dh-python's `_max_compatible`: with three or more release
/// segments the last is dropped and the new final segment bumped (`1.4.5` ->
/// `1.5`); with fewer, everything after the first is dropped and the first
/// bumped (`2.2` -> `3`, `1` -> `2`).
fn max_compatible_release(epoch: u64, release: &[u64]) -> debversion::Version {
    let keep = if release.len() >= 3 { 2 } else { 1 };
    bumped_release(epoch, &release[..keep.min(release.len())])
}

#[cfg(feature = "debian")]
/// Return the maximum version compatible with `version` under PEP 440's `~=`
/// operator, as a Debian version.
fn max_compatible_debian_version(version: &pep440_rs::Version) -> debversion::Version {
    max_compatible_release(version.epoch(), version.release())
}

#[cfg(feature = "debian")]
/// Convert Python version specifiers to Debian package version constraints.
///
/// # Arguments
/// * `pkg_name` - The name of the Debian package
/// * `version_specifiers` - The Python version specifiers (PEP 440)
///
/// # Returns
/// Debian package relations with appropriate version constraints
pub fn python_version_specifiers_to_debian(
    pkg_name: &str,
    version_specifiers: Option<&pep440_rs::VersionSpecifiers>,
) -> Relations {
    // Each Entry is ANDed with the others; the Relations within a single Entry
    // are ORed. Most operators contribute one or more single-relation (ANDed)
    // entries; `!=` needs an OR, which a multi-relation entry expresses.
    let mut entries: Vec<Entry> = vec![];
    let and = |entries: &mut Vec<Entry>, vc, v| {
        entries.push(Relation::new(pkg_name, Some((vc, v))).into());
    };
    if let Some(version_specifiers) = version_specifiers {
        for vs in version_specifiers.iter() {
            let version = vs.version();
            match vs.operator() {
                pep440_rs::Operator::NotEqual => {
                    // != V excludes exactly V: << V | >> V.
                    let deb_version = pep440_to_debian_version(version);
                    entries.push(
                        vec![
                            Relation::new(
                                pkg_name,
                                Some((VersionConstraint::LessThan, deb_version.clone())),
                            ),
                            Relation::new(
                                pkg_name,
                                Some((VersionConstraint::GreaterThan, deb_version)),
                            ),
                        ]
                        .into(),
                    );
                }
                pep440_rs::Operator::NotEqualStar => {
                    // != V.* excludes the whole [V, V.next) range: << V | >= V.next,
                    // where V.next bumps V's last release segment.
                    let lower = release_debian_version(version.epoch(), version.release());
                    let upper = bumped_release(version.epoch(), version.release());
                    entries.push(
                        vec![
                            Relation::new(pkg_name, Some((VersionConstraint::LessThan, lower))),
                            Relation::new(
                                pkg_name,
                                Some((VersionConstraint::GreaterThanEqual, upper)),
                            ),
                        ]
                        .into(),
                    );
                }
                pep440_rs::Operator::TildeEqual => {
                    // ~= V is >= V, << max_compatible(V).
                    and(
                        &mut entries,
                        VersionConstraint::GreaterThanEqual,
                        pep440_to_debian_version(version),
                    );
                    and(
                        &mut entries,
                        VersionConstraint::LessThan,
                        max_compatible_debian_version(version),
                    );
                }
                pep440_rs::Operator::EqualStar => {
                    // == V.* matches the [V, V.next) range: >= V, << V.next, where
                    // V.next bumps V's last release segment.
                    let lower = release_debian_version(version.epoch(), version.release());
                    let upper = bumped_release(version.epoch(), version.release());
                    and(&mut entries, VersionConstraint::GreaterThanEqual, lower);
                    and(&mut entries, VersionConstraint::LessThan, upper);
                }
                pep440_rs::Operator::Equal => {
                    // dh-python loosens an exact `==` to tolerate Debian revisions:
                    // >= V, << (V with its last number bumped and a trailing `~`).
                    let deb_version = pep440_to_debian_version(version);
                    let mut upper = deb_version.clone();
                    upper.upstream_version =
                        format!("{}~", bump_last_number(&upper.upstream_version));
                    and(
                        &mut entries,
                        VersionConstraint::GreaterThanEqual,
                        deb_version,
                    );
                    and(&mut entries, VersionConstraint::LessThan, upper);
                }
                o => {
                    let vc = match o {
                        pep440_rs::Operator::GreaterThanEqual => {
                            VersionConstraint::GreaterThanEqual
                        }
                        pep440_rs::Operator::GreaterThan => VersionConstraint::GreaterThan,
                        pep440_rs::Operator::LessThanEqual => VersionConstraint::LessThanEqual,
                        pep440_rs::Operator::LessThan => VersionConstraint::LessThan,
                        // ExactEqual (`===`) compares an arbitrary string; there is
                        // no meaningful Debian translation.
                        _ => continue,
                    };
                    and(&mut entries, vc, pep440_to_debian_version(version));
                }
            }
        }
        if entries.is_empty() {
            // Every specifier was dropped (e.g. only `===`); fall back to an
            // unversioned dependency on the package.
            entries.push(Relation::new(pkg_name, None).into());
        }
        Relations::from(entries)
    } else {
        Relations::from(vec![Relation::new(pkg_name, None).into()])
    }
}

fn find_python_version(marker: Vec<Vec<pep508_rs::MarkerExpression>>) -> Option<PythonVersion> {
    let mut major_version = None;
    let mut implementation = None;
    for expr in marker.iter().flat_map(|x| x.iter()) {
        match expr {
            pep508_rs::MarkerExpression::Version {
                key: pep508_rs::MarkerValueVersion::PythonVersion,
                specifier,
            } => {
                let version = specifier.version();
                major_version = Some(version.release()[0] as u32);
            }
            pep508_rs::MarkerExpression::String {
                key: pep508_rs::MarkerValueString::PlatformPythonImplementation,
                operator: pep508_rs::MarkerOperator::Equal,
                value,
            } => {
                if value.as_str() == "PyPy" {
                    implementation = Some("PyPy");
                }
            }
            _ => {}
        }
    }

    match (major_version, implementation) {
        (Some(2), None) => Some(PythonVersion::CPython2),
        (Some(3), None) | (None, None) => Some(PythonVersion::CPython3),
        (Some(3), Some("PyPy")) | (None, Some("PyPy")) => Some(PythonVersion::PyPy3),
        (Some(2), Some("PyPy")) => Some(PythonVersion::PyPy),
        _ => {
            log::warn!(
                "Unknown python implementation / version: {:?} {:?}",
                major_version,
                implementation
            );
            None
        }
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PythonPackageDependency {
    fn try_into_debian_dependency(
        &self,
        apt_mgr: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        let names = get_package_for_python_package(
            apt_mgr,
            &self.package(),
            find_python_version(self.0.marker.to_dnf()),
            self.0.version_or_url.as_ref(),
        );
        Some(names)
    }
}

#[cfg(feature = "debian")]
fn get_package_for_python_package(
    apt_mgr: &AptManager,
    package: &str,
    python_version: Option<PythonVersion>,
    version_or_url: Option<&pep508_rs::VersionOrUrl>,
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
        Some(PythonVersion::PyPy) => vec![pypy_regex],
        Some(PythonVersion::CPython2) => vec![cpython2_regex],
        Some(PythonVersion::CPython3) => vec![cpython3_regex],
        None => vec![cpython3_regex, cpython2_regex, pypy_regex],
        _ => unimplemented!(),
    };
    let names = apt_mgr
        .get_packages_for_paths(paths.iter().map(|x| x.as_str()).collect(), true, true)
        .unwrap();

    names
        .iter()
        .map(|name| {
            DebianDependency::from(python_version_specifiers_to_debian(
                name,
                if let Some(pep508_rs::VersionOrUrl::VersionSpecifier(specs)) = version_or_url {
                    Some(specs)
                } else {
                    None
                },
            ))
        })
        .collect()
}

#[cfg(any(feature = "debian", test))]
fn get_possible_python3_paths_for_python_object(mut object_path: &str) -> Vec<std::path::PathBuf> {
    let mut cpython3_regexes = vec![];
    loop {
        cpython3_regexes.extend([
            Path::new("/usr/lib/python3/dist\\-packages")
                .join(regex::escape(&object_path.replace('.', "/")))
                .join("__init__\\.py"),
            Path::new("/usr/lib/python3/dist\\-packages").join(format!(
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

#[cfg(feature = "debian")]
fn get_possible_pypy_paths_for_python_object(mut object_path: &str) -> Vec<std::path::PathBuf> {
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

#[cfg(feature = "debian")]
fn get_possible_python2_paths_for_python_object(mut object_path: &str) -> Vec<std::path::PathBuf> {
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

#[cfg(feature = "debian")]
fn get_package_for_python_object_path(
    apt_mgr: &AptManager,
    object_path: &str,
    python_version: Option<PythonVersion>,
    version_specifiers: Option<&pep440_rs::VersionSpecifiers>,
) -> Vec<DebianDependency> {
    // Try to find the most specific file
    let paths = match python_version {
        Some(PythonVersion::CPython3) => get_possible_python3_paths_for_python_object(object_path),
        Some(PythonVersion::CPython2) => get_possible_python2_paths_for_python_object(object_path),
        Some(PythonVersion::PyPy) => get_possible_pypy_paths_for_python_object(object_path),
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
        .map(|name| {
            DebianDependency::from(python_version_specifiers_to_debian(
                &name,
                version_specifiers,
            ))
        })
        .collect()
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PythonModuleDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        use std::str::FromStr;
        let specs = self.minimum_version.as_ref().map(|min_version| {
            std::iter::once(
                pep440_rs::VersionSpecifier::from_pattern(
                    pep440_rs::Operator::GreaterThanEqual,
                    pep440_rs::VersionPattern::verbatim(
                        pep440_rs::Version::from_str(min_version).unwrap(),
                    ),
                )
                .unwrap(),
            )
            .collect()
        });
        Some(get_package_for_python_object_path(
            apt,
            &self.module,
            self.python_version,
            specs.as_ref(),
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
/// A dependency on Python itself.
///
/// This represents a dependency on the Python interpreter or development files.
pub struct PythonDependency {
    /// The minimum Python version required, if any.
    pub min_version: Option<String>,
}

impl PythonDependency {
    /// Create a new Python dependency with an optional minimum version.
    ///
    /// # Arguments
    /// * `min_version` - The minimum Python version required (e.g., "3.8")
    ///
    /// # Returns
    /// A new PythonDependency
    pub fn new(min_version: Option<&str>) -> Self {
        Self {
            min_version: min_version.map(|s| s.to_string()),
        }
    }

    /// Create a simple Python dependency with no version constraints.
    ///
    /// # Returns
    /// A new PythonDependency with no version constraints
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

#[cfg(test)]
mod python_dep_tests {
    use super::*;

    #[test]
    fn test_python_dependency_new() {
        let dependency = PythonDependency::new(Some("3.6"));
        assert_eq!(dependency.min_version, Some("3.6".to_string()));
    }

    #[test]
    fn test_python_dependency_simple() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.min_version, None);
    }

    #[test]
    fn test_python_dependency_family() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.family(), "python");
    }

    #[test]
    fn test_python_dependency_executable_python3() {
        let dependency = PythonDependency::new(Some("3.6"));
        assert_eq!(dependency.executable(), "python3");
    }

    #[test]
    fn test_python_dependency_executable_python2() {
        let dependency = PythonDependency::new(Some("2.7"));
        assert_eq!(dependency.executable(), "python");
    }

    #[test]
    fn test_python_dependency_executable_default() {
        let dependency = PythonDependency::simple();
        assert_eq!(dependency.executable(), "python3");
    }

    #[test]
    fn test_python_dependency_from_specs() {
        use std::str::FromStr;
        let specs = pep440_rs::VersionSpecifiers::from_str(">=3.6").unwrap();
        let dependency = PythonDependency::from(&specs);
        // The actual version might be "3.6" or "3.6.0" depending on the pep440_rs version, so we just check that it contains "3.6"
        assert!(dependency.min_version.is_some());
        assert!(dependency.min_version.as_ref().unwrap().contains("3.6"));
    }

    #[test]
    fn test_python_dependency_from_specs_no_version() {
        use std::str::FromStr;
        let specs = pep440_rs::VersionSpecifiers::from_str("==3.6").unwrap();
        let dependency = PythonDependency::from(&specs);
        assert_eq!(dependency.min_version, None);
    }
}

#[cfg(all(test, feature = "debian"))]
mod version_conversion_tests {
    use super::*;
    use std::str::FromStr;

    fn pep440(s: &str) -> pep440_rs::Version {
        pep440_rs::Version::from_str(s).unwrap()
    }

    fn deb(s: &str) -> debversion::Version {
        s.parse().unwrap()
    }

    #[test]
    fn test_plain_release() {
        assert_eq!(deb("1.2.3"), pep440_to_debian_version(&pep440("1.2.3")));
    }

    #[test]
    fn test_epoch() {
        let v = pep440_to_debian_version(&pep440("1!2.0"));
        assert_eq!(Some(1), v.epoch);
        assert_eq!("2.0", v.upstream_version);
    }

    #[test]
    fn test_prerelease() {
        assert_eq!(
            "1.0~a1",
            pep440_to_debian_version(&pep440("1.0a1")).upstream_version
        );
        assert_eq!(
            "1.0~b2",
            pep440_to_debian_version(&pep440("1.0b2")).upstream_version
        );
        assert_eq!(
            "1.0~rc3",
            pep440_to_debian_version(&pep440("1.0rc3")).upstream_version
        );
    }

    #[test]
    fn test_dev_release() {
        // dh-python's PEP 440 mode leaves dev releases verbatim.
        assert_eq!(
            "1.0.dev1",
            pep440_to_debian_version(&pep440("1.0.dev1")).upstream_version
        );
    }

    #[test]
    fn test_post_release() {
        assert_eq!(
            "1.0.post1",
            pep440_to_debian_version(&pep440("1.0.post1")).upstream_version
        );
    }

    #[test]
    fn test_local_version() {
        // Structural output keeps the local segment intact, unlike dh-python's
        // regex which would mangle the 'b' in 'ubuntu'.
        assert_eq!(
            "1.0+ubuntu.1",
            pep440_to_debian_version(&pep440("1.0+ubuntu.1")).upstream_version
        );
    }

    #[test]
    fn test_dev_of_prerelease() {
        // Only the pre-release tag gets the leading '~'; dev stays verbatim.
        assert_eq!(
            "1.0~a1.dev1",
            pep440_to_debian_version(&pep440("1.0a1.dev1")).upstream_version
        );
    }

    #[test]
    fn test_prerelease_sorts_before_release() {
        // The one ordering guarantee dh-python's mapping provides: a pre-release
        // sorts before the final release, and a < b < rc.
        let order = ["1.0~a1", "1.0~b1", "1.0~rc1", "1.0"];
        let pre = ["1.0a1", "1.0b1", "1.0rc1", "1.0"]
            .iter()
            .map(|s| pep440_to_debian_version(&pep440(s)))
            .collect::<Vec<_>>();
        for (got, want) in pre.iter().zip(order) {
            assert_eq!(want, got.upstream_version);
        }
        for pair in pre.windows(2) {
            assert!(
                pair[0] < pair[1],
                "{:?} should sort before {:?}",
                pair[0],
                pair[1]
            );
        }
    }

    #[test]
    fn test_roundtrip() {
        for s in [
            "1.2.3",
            "1!2.0",
            "1.0a1",
            "1.0b2",
            "1.0rc3",
            "1.0.dev1",
            "1.0a1.dev1",
            "1.0.post1",
        ] {
            let deb = pep440_to_debian_version(&pep440(s));
            let back = debian_to_pep440_version(&deb).unwrap();
            assert_eq!(pep440(s), back, "roundtrip failed for {s}");
        }
    }

    #[test]
    fn test_debian_to_pep440_ignores_revision() {
        assert_eq!(
            pep440("1.2.3"),
            debian_to_pep440_version(&deb("1.2.3-1")).unwrap()
        );
    }

    #[test]
    fn test_debian_to_pep440_rejects_unknown_tilde() {
        assert_eq!(None, debian_to_pep440_version(&deb("1.0~foo1")));
    }

    #[test]
    fn test_specifiers_greater_than_equal() {
        let specs = pep440_rs::VersionSpecifiers::from_str(">=1.0a1").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (>= 1.0~a1)", rels.to_string());
    }

    #[test]
    fn test_specifiers_tilde_equal() {
        let specs = pep440_rs::VersionSpecifiers::from_str("~=1.4.5").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (>= 1.4.5), foo (<< 1.5)", rels.to_string());
    }

    #[test]
    fn test_specifiers_tilde_equal_two_segments() {
        let specs = pep440_rs::VersionSpecifiers::from_str("~=2.2").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (>= 2.2), foo (<< 3)", rels.to_string());
    }

    #[test]
    fn test_specifiers_equal_star() {
        // == 1.4.* matches [1.4, 1.5). The lower bound is the bare 1.4 so that
        // version 1.4 itself matches (1.4 sorts before 1.4.0 in Debian).
        let specs = pep440_rs::VersionSpecifiers::from_str("==1.4.*").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (>= 1.4), foo (<< 1.5)", rels.to_string());
    }

    #[test]
    fn test_specifiers_exact_equal_loosened() {
        // dh-python loosens `== V` to a range so Debian revisions still match.
        let specs = pep440_rs::VersionSpecifiers::from_str("==1.4").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (>= 1.4), foo (<< 1.5~)", rels.to_string());
    }

    #[test]
    fn test_specifiers_not_equal() {
        // != V excludes exactly V, expressed as an OR.
        let specs = pep440_rs::VersionSpecifiers::from_str("!=1.4").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (<< 1.4) | foo (>> 1.4)", rels.to_string());
    }

    #[test]
    fn test_specifiers_not_equal_star() {
        // != V.* excludes [1.4, 1.5): << 1.4 | >= 1.5. The lower bound is the
        // bare 1.4 so that 1.4 itself is excluded.
        let specs = pep440_rs::VersionSpecifiers::from_str("!=1.4.*").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!("foo (<< 1.4) | foo (>= 1.5)", rels.to_string());
    }

    #[test]
    fn test_specifiers_combined_with_not_equal() {
        // A `>=` AND a `!=` combine: the != becomes an ORed entry alongside it.
        let specs = pep440_rs::VersionSpecifiers::from_str(">=1.0,!=1.4").unwrap();
        let rels = python_version_specifiers_to_debian("foo", Some(&specs));
        assert_eq!(
            "foo (>= 1.0), foo (<< 1.4) | foo (>> 1.4)",
            rels.to_string()
        );
    }

    #[test]
    fn test_bump_last_number() {
        assert_eq!("1.5", bump_last_number("1.4"));
        assert_eq!("1.4.6", bump_last_number("1.4.5"));
        assert_eq!("1.0~a2", bump_last_number("1.0~a1"));
        // No trailing digit run after the local segment: the release digit bumps.
        assert_eq!("1.1+local", bump_last_number("1.0+local"));
        // No digits at all: returned unchanged.
        assert_eq!("abc", bump_last_number("abc"));
    }

    #[test]
    fn test_specifiers_none() {
        let rels = python_version_specifiers_to_debian("foo", None);
        assert_eq!("foo", rels.to_string());
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
                PythonVersion::default().executable().to_string(),
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
            if specifier.operator() == &pep440_rs::Operator::GreaterThanEqual {
                return Self {
                    min_version: Some(specifier.version().to_string()),
                };
            }
        }
        Self { min_version: None }
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::FromDebianDependency for PythonDependency {
    fn from_debian_dependency(dependency: &DebianDependency) -> Option<Box<dyn Dependency>> {
        let (name, min_version) =
            crate::dependencies::debian::extract_simple_min_version(dependency)?;
        if name == "python" || name == "python3" {
            // A present-but-unconvertible version means we cannot faithfully
            // represent the dependency; give up rather than dropping the bound.
            let min_version = match min_version {
                Some(v) => Some(debian_to_pep440_version(&v)?.to_string()),
                None => None,
            };
            Some(Box::new(PythonDependency { min_version }))
        } else {
            None
        }
    }
}

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for PythonDependency {
    fn try_into_debian_dependency(
        &self,
        _apt: &crate::debian::apt::AptManager,
    ) -> Option<Vec<DebianDependency>> {
        let mut deps = vec![];
        if let Some(min_version) = &self.min_version {
            use std::str::FromStr;
            let pep440 = pep440_rs::Version::from_str(min_version).ok()?;
            let deb_version = pep440_to_debian_version(&pep440);
            let pkg = if min_version.starts_with("2") {
                "python"
            } else {
                "python3"
            };
            deps.push(
                crate::dependencies::debian::DebianDependency::new_with_min_version(
                    pkg,
                    &deb_version,
                ),
            );
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
        use std::path::PathBuf;

        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/dulwich\\.py"),
                PathBuf::from(
                    "/usr/lib/python3\\.[0-9]+/lib\\-dynload/dulwich\\.cpython\\-.*\\.so"
                ),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/dulwich/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("dulwich"),
        );
        assert_eq!(
            vec![
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/foo\\.py"),
                PathBuf::from(
                    "/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo/foo\\.cpython\\-.*\\.so"
                ),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/foo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo/__init__\\.py"),
                PathBuf::from("/usr/lib/python3/dist\\-packages/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/lib\\-dynload/cleo\\.cpython\\-.*\\.so"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo\\.py"),
                PathBuf::from("/usr/lib/python3\\.[0-9]+/cleo/__init__\\.py"),
            ],
            get_possible_python3_paths_for_python_object("cleo.foo"),
        );
    }
}
