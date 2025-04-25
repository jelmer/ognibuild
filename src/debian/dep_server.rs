//! Dependency server integration for Debian packages.
//!
//! This module provides functionality for resolving dependencies using
//! a remote dependency server that can translate generic dependencies
//! into Debian package dependencies.

use crate::debian::apt::AptManager;
use crate::dependencies::debian::DebianDependency;
use crate::dependency::Dependency;
use crate::installer::{Error, Explanation, InstallationScope, Installer};
use crate::session::Session;
use reqwest::StatusCode;
use tokio::runtime::Runtime;
use url::Url;

/// Resolve a requirement to an APT requirement with a dep server.
///
/// # Arguments
/// * `url` - Dep server URL
/// * `req` - Dependency to resolve
///
/// # Returns
/// List of APT requirements.
async fn resolve_apt_requirement_dep_server(
    url: &url::Url,
    dep: &dyn Dependency,
) -> Result<Option<DebianDependency>, Error> {
    let client = reqwest::Client::new();
    let response = client
        .post(url.join("resolve-apt").unwrap())
        .json(&serde_json::json!( {
            "requirement": {
                // TODO: Use the actual dependency
            }
        }))
        .send()
        .await
        .unwrap();

    match response.status() {
        StatusCode::NOT_FOUND => {
            if response
                .headers()
                .get("Reason")
                .map(|x| x.to_str().unwrap())
                == Some("family-unknown")
            {
                return Err(Error::UnknownDependencyFamily);
            }
            Ok(None)
        }
        StatusCode::OK => {
            let body = response.json::<DebianDependency>().await.unwrap();
            Ok(Some(body))
        }
        _ => {
            panic!("Unexpected response status: {}", response.status());
        }
    }
}

/// Installer that uses a dependency server to resolve and install dependencies.
///
/// This installer connects to a remote dependency server that can translate
/// generic dependencies into Debian package dependencies and then installs them
/// using APT.
pub struct DepServerAptInstaller<'a> {
    /// APT manager for package operations
    apt: AptManager<'a>,
    /// URL of the dependency server
    dep_server_url: Url,
}

impl<'a> DepServerAptInstaller<'a> {
    /// Create a new DepServerAptInstaller with the given APT manager and server URL.
    ///
    /// # Arguments
    /// * `apt` - APT manager to use for installing dependencies
    /// * `dep_server_url` - URL of the dependency server
    ///
    /// # Returns
    /// A new DepServerAptInstaller instance
    pub fn new(apt: AptManager<'a>, dep_server_url: &Url) -> Self {
        Self {
            apt,
            dep_server_url: dep_server_url.clone(),
        }
    }

    /// Create a new DepServerAptInstaller from a session and server URL.
    ///
    /// # Arguments
    /// * `session` - Session to use for running commands
    /// * `dep_server_url` - URL of the dependency server
    ///
    /// # Returns
    /// A new DepServerAptInstaller instance
    pub fn from_session(session: &'a dyn Session, dep_server_url: &'_ Url) -> Self {
        let apt = AptManager::from_session(session);
        Self::new(apt, dep_server_url)
    }

    /// Resolve a dependency to a Debian package dependency using the dependency server.
    ///
    /// # Arguments
    /// * `req` - Generic dependency to resolve
    ///
    /// # Returns
    /// Some(DebianDependency) if the server could resolve it, None if not found,
    /// or Error if there was a problem communicating with the server
    pub fn resolve(&self, req: &dyn Dependency) -> Result<Option<DebianDependency>, Error> {
        let rt = Runtime::new().unwrap();
        match rt.block_on(resolve_apt_requirement_dep_server(
            &self.dep_server_url,
            req,
        )) {
            Ok(deps) => Ok(deps),
            Err(o) => {
                log::warn!("Falling back to resolving error locally");
                Err(Error::Other(o.to_string()))
            }
        }
    }
}

/// Implementation of the Installer trait for DepServerAptInstaller.
impl<'a> Installer for DepServerAptInstaller<'a> {
    fn install(
        &self,
        dep: &dyn Dependency,
        scope: crate::installer::InstallationScope,
    ) -> Result<(), Error> {
        match scope {
            InstallationScope::User => {
                return Err(Error::UnsupportedScope(scope));
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        let dep = self.resolve(dep)?;

        if let Some(dep) = dep {
            match self
                .apt
                .satisfy(vec![crate::debian::apt::SatisfyEntry::Required(
                    dep.relation_string(),
                )]) {
                Ok(_) => {}
                Err(e) => {
                    return Err(Error::Other(e.to_string()));
                }
            }
            Ok(())
        } else {
            Err(Error::UnknownDependencyFamily)
        }
    }

    fn explain(
        &self,
        dep: &dyn Dependency,
        scope: crate::installer::InstallationScope,
    ) -> Result<crate::installer::Explanation, Error> {
        match scope {
            InstallationScope::User => {
                return Err(Error::UnsupportedScope(scope));
            }
            InstallationScope::Global => {}
            InstallationScope::Vendor => {
                return Err(Error::UnsupportedScope(scope));
            }
        }
        let dep = self.resolve(dep)?;

        let dep = dep.ok_or_else(|| Error::UnknownDependencyFamily)?;

        let apt_deb_str = dep.relation_string();
        let cmd = self.apt.satisfy_command(vec![apt_deb_str.as_str()]);
        Ok(Explanation {
            message: format!(
                "Install {}",
                dep.package_names()
                    .iter()
                    .map(|x| x.as_str())
                    .collect::<Vec<_>>()
                    .join(", ")
            ),
            command: Some(cmd.iter().map(|s| s.to_string()).collect()),
        })
    }
}
