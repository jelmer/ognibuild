use crate::dependencies::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct XmlEntityDependency {
    url: String,
}

impl XmlEntityDependency {
    pub fn new(url: &str) -> Self {
        Self {
            url: url.to_string(),
        }
    }
}

impl Dependency for XmlEntityDependency {
    fn family(&self) -> &'static str {
        "xml-entity"
    }

    fn present(&self, session: &dyn Session) -> bool {
        // Check if the entity is defined in the local XML catalog
        session
            .command(vec!["xmlcatalog", "--noout", "--resolve", &self.url])
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

pub const XML_ENTITY_URL_MAP: &[(&str, &str)] = &[(
    "http://www.oasis-open.org/docbook/xml/",
    "/usr/share/xml/docbook/schema/dtd/",
)];

#[cfg(feature = "debian")]
impl crate::dependencies::debian::IntoDebianDependency for XmlEntityDependency {
    fn try_into_debian_dependency(
        &self,
        apt: &crate::debian::apt::AptManager,
    ) -> std::option::Option<std::vec::Vec<crate::dependencies::debian::DebianDependency>> {
        let path = XML_ENTITY_URL_MAP.iter().find_map(|(url, path)| {
            self.url
                .strip_prefix(url)
                .map(|rest| format!("{}{}", path, rest))
        });

        path.as_ref()?;

        Some(
            apt.get_packages_for_paths(vec![path.as_ref().unwrap()], false, false)
                .unwrap()
                .iter()
                .map(|p| crate::dependencies::debian::DebianDependency::simple(p.as_str()))
                .collect(),
        )
    }
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingXmlEntity {
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(XmlEntityDependency::new(&self.url)))
    }
}
