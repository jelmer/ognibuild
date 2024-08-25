use crate::dependency::{Error, Dependency, Resolver};
use tokio::runtime::Runtime;
use crate::dependencies::debian::{DebianDependency, TieBreaker};
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
    url: &url::Url, dep: &Dependency
) -> Result<Vec<DebianDependency>, Error> {
    let client = reqwest::Client::new();
    let response = client
        .post(url.join("resolve-apt").unwrap())
        .json(serde_json::json!( {
            "requirement": {
                "family": dep.family(),
                "details": dep.json()
            }
        }))
        .send()
        .await?;

    match response.status() {
        404 => {
            if response.headers().get("Reason") == Some("family-unknown") {
                return Err(Error::UnknownDependencyFamily);
            }
            panic!("Unexpected 404 response");
        }
        200 => {
            let body = response.json::<Vec<DebianDependency>>().await?;
            Ok(body)
        }
        _ => {
            panic!("Unexpected response status: {}", response.status());
        }
    }
}

pub DepServerAptResolver {
    apt: AptManager,
    dep_server_url: Url,
    tie_breakers: Vec<Box<dyn TieBreaker>>,
}

impl DepServerAptResolver {
    pub fn new(apt: AptManager, dep_server_url: Url, tie_breakers: Vec<Box<dyn TieBreaker>>) -> Self {
        Self {
            apt,
            dep_server_url,
            tie_breakers,
        }
    }

    pub fn from_session(session: &Session, dep_server_url: Url, tie_breakers: Vec<Box<dyn TieBreaker>>) -> Self {
        Self {
            apt: AptManager::from_session(session),
            dep_server_url,
            tie_breakers,
        }
    }
}

impl Resolver for DepServerAptResolver {
    type Target = DebianDependency;
    fn resolve(&self, req: &Requirement) -> Result<Vec<DebianDependency>, Error> {
        let rt = Runtime::new().unwrap();
        match rt.block_on(resolve_apt_requirement_dep_server(&self.dep_server_url, req)) {
            Ok(deps) => Ok(deps),
            Err(_) => {
                log::warn!("Falling back to resolving error locally");
                self.apt.resolve_all(req, &self.tie_breakers)
            }
        }
    }
}
