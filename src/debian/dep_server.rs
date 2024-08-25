use crate::dependency::{Error, Dependency, Resolver};
use crate::debian::apt::AptManager;
use tokio::runtime::Runtime;
use crate::dependencies::debian::{DebianDependency, TieBreaker};
use crate::session::Session;
use url::Url;
use reqwest::StatusCode;

/// Resolve a requirement to an APT requirement with a dep server.
///
/// # Arguments
/// * `url` - Dep server URL
/// * `req` - Dependency to resolve
///
/// # Returns
/// List of APT requirements.
async fn resolve_apt_requirement_dep_server(
    url: &url::Url, dep: &dyn Dependency
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
        .await.unwrap();

    match response.status() {
        StatusCode::NOT_FOUND => {
            if response.headers().get("Reason").map(|x| x.to_str().unwrap()) == Some("family-unknown") {
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

pub struct DepServerAptResolver<'a> {
    apt: AptManager<'a>,
    dep_server_url: Url,
    tie_breakers: Vec<Box<dyn TieBreaker>>,
}

impl<'a> DepServerAptResolver<'a> {
    pub fn new(apt: AptManager<'a>, dep_server_url: Url, tie_breakers: Vec<Box<dyn TieBreaker>>) -> Self {
        Self {
            apt,
            dep_server_url,
            tie_breakers,
        }
    }

    pub fn from_session(session: &'a dyn Session, dep_server_url: Url, tie_breakers: Vec<Box<dyn TieBreaker>>) -> Self {
        Self {
            apt: AptManager::from_session(session),
            dep_server_url,
            tie_breakers,
        }
    }
}

impl<'a> Resolver for DepServerAptResolver<'a> {
    type Target = DebianDependency;
    fn resolve(&self, req: &dyn Dependency) -> Result<Option<DebianDependency>, Error> {
        let rt = Runtime::new().unwrap();
        match rt.block_on(resolve_apt_requirement_dep_server(&self.dep_server_url, req)) {
            Ok(deps) => Ok(deps),
            Err(o) => {
                log::warn!("Falling back to resolving error locally");
                Err(Error::Other(o.to_string()))
            }
        }
    }
}
