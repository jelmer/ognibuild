use crate::dependency::Dependency;
use crate::session::Session;
use serde::{Deserialize, Serialize};
use std::io::BufRead;

/// Dependency on an Autoconf macro.
///
/// This represents a dependency on a specific Autoconf macro that can be
/// used in configure.ac files.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct AutoconfMacroDependency {
    /// Name of the Autoconf macro
    pub macro_name: String,
}

impl AutoconfMacroDependency {
    /// Create a new AutoconfMacroDependency.
    ///
    /// # Arguments
    /// * `macro_name` - Name of the Autoconf macro
    ///
    /// # Returns
    /// A new AutoconfMacroDependency instance
    pub fn new(macro_name: &str) -> Self {
        Self {
            macro_name: macro_name.to_string(),
        }
    }
}

impl Dependency for AutoconfMacroDependency {
    /// Returns the family name for this dependency type.
    ///
    /// # Returns
    /// The string "autoconf-macro"
    fn family(&self) -> &'static str {
        "autoconf-macro"
    }

    /// Checks if the Autoconf macro is present in the system.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Checks if the Autoconf macro is present in the project.
    ///
    /// # Arguments
    /// * `_session` - The session in which to check
    ///
    /// # Returns
    /// This method is not implemented yet and will panic if called
    fn project_present(&self, _session: &dyn Session) -> bool {
        todo!()
    }

    /// Returns this dependency as a trait object.
    ///
    /// # Returns
    /// Reference to this object as a trait object
    fn as_any(&self) -> &dyn std::any::Any {
        self
    }
}

/// Create a regular expression to find a macro definition in M4 files.
///
/// This function generates a regex pattern that can match various ways a macro
/// might be defined in M4 files (via AC_DEFUN, AU_ALIAS, or m4_copy).
///
/// # Arguments
/// * `macro` - Name of the Autoconf macro to search for
///
/// # Returns
/// Regular expression string pattern for finding the macro definition
pub fn m4_macro_regex(r#macro: &str) -> String {
    let defun_prefix = regex::escape(format!("AC_DEFUN([{}],", r#macro).as_str());
    let au_alias_prefix = regex::escape(format!("AU_ALIAS([{}],", r#macro).as_str());
    let m4_copy = format!(r"m4_copy\(.*,\s*\[{}\]\)", regex::escape(r#macro));
    [
        "(",
        &defun_prefix,
        "|",
        &au_alias_prefix,
        "|",
        &m4_copy,
        ")",
    ]
    .concat()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_autoconf_macro_dependency_new() {
        let dependency = AutoconfMacroDependency::new("PKG_CHECK_MODULES");
        assert_eq!(dependency.macro_name, "PKG_CHECK_MODULES");
    }

    #[test]
    fn test_autoconf_macro_dependency_family() {
        let dependency = AutoconfMacroDependency::new("PKG_CHECK_MODULES");
        assert_eq!(dependency.family(), "autoconf-macro");
    }

    #[test]
    fn test_m4_macro_regex() {
        let regex = m4_macro_regex("PKG_CHECK_MODULES");

        // Test AC_DEFUN matching
        assert!(regex::Regex::new(&regex)
            .unwrap()
            .is_match("AC_DEFUN([PKG_CHECK_MODULES],"));

        // Test AU_ALIAS matching
        assert!(regex::Regex::new(&regex)
            .unwrap()
            .is_match("AU_ALIAS([PKG_CHECK_MODULES],"));

        // Test m4_copy matching
        assert!(regex::Regex::new(&regex)
            .unwrap()
            .is_match("m4_copy([SOME_MACRO], [PKG_CHECK_MODULES])"));

        // Test negative case
        assert!(!regex::Regex::new(&regex)
            .unwrap()
            .is_match("PKG_CHECK_MODULES"));
    }
}

/// Find a local M4 macro file that contains the definition of a given macro.
///
/// Searches in `/usr/share/aclocal` for files containing the definition
/// of the specified macro.
///
/// # Arguments
/// * `macro` - Name of the Autoconf macro to search for
///
/// # Returns
/// Path to the M4 file containing the macro definition, or None if not found
pub fn find_local_m4_macro(r#macro: &str) -> Option<String> {
    // TODO(jelmer): Query some external service that can search all binary packages?
    let p = regex::Regex::new(&m4_macro_regex(r#macro)).unwrap();
    for entry in std::fs::read_dir("/usr/share/aclocal").unwrap() {
        let entry = entry.unwrap();
        if !entry.metadata().unwrap().is_file() {
            continue;
        }
        let f = std::fs::File::open(entry.path()).unwrap();
        let reader = std::io::BufReader::new(f);
        for line in reader.lines() {
            if p.find(line.unwrap().as_str()).is_some() {
                return Some(entry.path().to_str().unwrap().to_string());
            }
        }
    }
    None
}

impl crate::buildlog::ToDependency for buildlog_consultant::problems::common::MissingAutoconfMacro {
    /// Convert a MissingAutoconfMacro problem to a Dependency.
    ///
    /// # Returns
    /// An AutoconfMacroDependency boxed as a Dependency trait object
    fn to_dependency(&self) -> Option<Box<dyn Dependency>> {
        Some(Box::new(AutoconfMacroDependency::new(&self.r#macro)))
    }
}
