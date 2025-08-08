//! Support for XML entity dependencies.
//!
//! This module provides functionality for working with XML entity dependencies,
//! including checking if entities are defined in the local XML catalog and
//! mapping between URLs and filesystem paths.

use crate::dependencies::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
/// A dependency on an XML entity, such as a DocBook DTD.
///
/// This represents a dependency on an XML entity, which is typically resolved
/// through an XML catalog.
pub struct XmlEntityDependency {
    url: String,
}

impl XmlEntityDependency {
    /// Create a new XML entity dependency with the specified URL.
    ///
    /// # Arguments
    /// * `url` - The URL of the XML entity
    ///
    /// # Returns
    /// A new XmlEntityDependency
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

/// Mapping between XML entity URLs and their filesystem locations.
///
/// This constant maps from entity URLs to their corresponding filesystem paths,
/// which is used to locate entities when resolving dependencies.
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

#[cfg(test)]
mod tests {
    use super::*;
    use std::any::Any;

    #[test]
    fn test_xml_entity_dependency_new() {
        let url = "http://www.oasis-open.org/docbook/xml/4.5/docbookx.dtd";
        let dependency = XmlEntityDependency::new(url);
        assert_eq!(dependency.url, url);
    }

    #[test]
    fn test_xml_entity_dependency_family() {
        let dependency = XmlEntityDependency::new("http://www.example.com/entity");
        assert_eq!(dependency.family(), "xml-entity");
    }

    #[test]
    fn test_xml_entity_dependency_as_any() {
        let dependency = XmlEntityDependency::new("http://www.example.com/entity");
        let any_dep: &dyn Any = dependency.as_any();
        assert!(any_dep.downcast_ref::<XmlEntityDependency>().is_some());
    }

    #[test]
    fn test_xml_entity_url_map() {
        assert!(XML_ENTITY_URL_MAP
            .iter()
            .any(|(url, _)| *url == "http://www.oasis-open.org/docbook/xml/"));

        // Test that the URL map can be used to transform URLs
        let input_url = "http://www.oasis-open.org/docbook/xml/4.5/docbookx.dtd";
        let expected_path = "/usr/share/xml/docbook/schema/dtd/4.5/docbookx.dtd";

        let transformed = XML_ENTITY_URL_MAP.iter().find_map(|(url, path)| {
            input_url
                .strip_prefix(url)
                .map(|rest| format!("{}{}", path, rest))
        });

        assert_eq!(transformed, Some(expected_path.to_string()));
    }
}
