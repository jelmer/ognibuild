//! Context for working with Debian packages.
//!
//! This module provides a context for operations on Debian packages,
//! including editing, committing changes, and managing dependencies.

use crate::dependencies::debian::DebianDependency;
use breezyshim::commit::PyCommitReporter;
use breezyshim::debian::debcommit::debcommit;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
pub use buildlog_consultant::sbuild::Phase;
use debian_analyzer::abstract_control::AbstractControlEditor;
use debian_analyzer::editor::{Editor, EditorError, Marshallable, MutableTreeEdit, TreeEditor};
use std::path::{Path, PathBuf};

/// Errors that can occur when working with Debian packages.
#[derive(Debug)]
pub enum Error {
    /// Circular dependency detected.
    CircularDependency(String),
    /// No source stanza found in debian/control.
    MissingSource,
    /// Error from breezyshim.
    BrzError(BrzError),
    /// Error from debian_analyzer editor.
    EditorError(debian_analyzer::editor::EditorError),
    /// I/O error when accessing files.
    IoError(std::io::Error),
    /// Invalid field value in control file.
    InvalidField(String, String),
}

impl From<BrzError> for Error {
    fn from(e: BrzError) -> Self {
        Error::BrzError(e)
    }
}

impl From<debian_analyzer::editor::EditorError> for Error {
    fn from(e: debian_analyzer::editor::EditorError) -> Self {
        Error::EditorError(e)
    }
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl From<Error> for crate::fix_build::InterimError<Error> {
    fn from(e: Error) -> crate::fix_build::InterimError<Error> {
        crate::fix_build::InterimError::Other(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Error::CircularDependency(pkg) => write!(f, "Circular dependency on {}", pkg),
            Error::MissingSource => write!(f, "No source stanza"),
            Error::BrzError(e) => write!(f, "{}", e),
            Error::EditorError(e) => write!(f, "{}", e),
            Error::IoError(e) => write!(f, "{}", e),
            Error::InvalidField(field, e) => write!(f, "Invalid field {}: {}", field, e),
        }
    }
}

impl std::error::Error for Error {}

/// Context for working with Debian packages.
///
/// This structure provides methods for modifying Debian package files,
/// committing changes, and managing dependencies.
pub struct DebianPackagingContext {
    /// Working tree containing the package source.
    pub tree: GenericWorkingTree,
    /// Path within the tree where the package is located.
    pub subpath: PathBuf,
    /// Committer information (name, email).
    pub committer: (String, String),
    /// Whether to update the changelog during commits.
    pub update_changelog: bool,
    /// Optional reporter for commit operations.
    pub commit_reporter: Option<Box<dyn PyCommitReporter>>,
}

impl DebianPackagingContext {
    /// Create a new Debian packaging context.
    ///
    /// # Arguments
    /// * `tree` - Working tree containing the package source
    /// * `subpath` - Path within the tree where the package is located
    /// * `committer` - Optional committer information (name, email)
    /// * `update_changelog` - Whether to update the changelog during commits
    /// * `commit_reporter` - Optional reporter for commit operations
    ///
    /// # Returns
    /// A new DebianPackagingContext instance
    pub fn new(
        tree: GenericWorkingTree,
        subpath: &Path,
        committer: Option<(String, String)>,
        update_changelog: bool,
        commit_reporter: Option<Box<dyn PyCommitReporter>>,
    ) -> Self {
        Self {
            tree,
            subpath: subpath.to_path_buf(),
            committer: committer.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap()),
            update_changelog,
            commit_reporter,
        }
    }

    /// Check if a file exists in the package tree.
    ///
    /// # Arguments
    /// * `path` - Path to check
    ///
    /// # Returns
    /// true if the file exists, false otherwise
    pub fn has_filename(&self, path: &Path) -> bool {
        self.tree.has_filename(&self.subpath.join(path))
    }

    /// Get the absolute path of a file in the package tree.
    ///
    /// # Arguments
    /// * `path` - Relative path within the package
    ///
    /// # Returns
    /// Absolute path to the file
    pub fn abspath(&self, path: &Path) -> PathBuf {
        self.tree.abspath(&self.subpath.join(path)).unwrap()
    }

    /// Create an editor for a file in the package tree.
    ///
    /// # Arguments
    /// * `path` - Path to the file to edit
    /// * `allow_generated` - Whether to allow editing generated files
    ///
    /// # Returns
    /// A TreeEditor for the specified file
    pub fn edit_file<P: Marshallable>(
        &self,
        path: &std::path::Path,
        allow_generated: bool,
    ) -> Result<TreeEditor<'_, P>, EditorError> {
        let path = self.subpath.join(path);
        self.tree.edit_file(&path, allow_generated, true)
    }

    /// Commit changes to the package tree.
    ///
    /// # Arguments
    /// * `summary` - Commit message summary
    /// * `update_changelog` - Whether to update the changelog (overrides context setting)
    ///
    /// # Returns
    /// Ok(true) if changes were committed, Ok(false) if no changes to commit, Error otherwise
    pub fn commit(&self, summary: &str, update_changelog: Option<bool>) -> Result<bool, Error> {
        let update_changelog = update_changelog.unwrap_or(self.update_changelog);

        let committer = format!("{} <{}>", self.committer.0, self.committer.1);

        let lock_write = self.tree.lock_write();
        let r = if update_changelog {
            let mut cl = self
                .edit_file::<debian_changelog::ChangeLog>(Path::new("debian/changelog"), false)?;
            cl.auto_add_change(&[summary], self.committer.clone(), None, None);
            cl.commit()?;

            debcommit(
                &self.tree,
                Some(&committer),
                &self.subpath,
                None,
                self.commit_reporter.as_deref(),
                None,
            )
        } else {
            let mut builder = self
                .tree
                .build_commit()
                .message(summary)
                .committer(&committer);

            if !self.subpath.as_os_str().is_empty() {
                builder = builder.specific_files(&[&self.subpath]);
            }
            if let Some(commit_reporter) = self.commit_reporter.as_ref() {
                builder = builder.reporter(commit_reporter.as_ref());
            }
            builder.commit()
        };

        std::mem::drop(lock_write);

        match r {
            Ok(_) => Ok(true),
            Err(BrzError::PointlessCommit) => Ok(false),
            Err(e) => Err(e.into()),
        }
    }

    /// Add a dependency to the package.
    ///
    /// # Arguments
    /// * `phase` - Build phase for the dependency
    /// * `requirement` - Debian dependency to add
    ///
    /// # Returns
    /// Ok(true) if dependency was added, Ok(false) if already present, Error otherwise
    pub fn add_dependency(
        &self,
        phase: &Phase,
        requirement: &DebianDependency,
    ) -> Result<bool, Error> {
        match phase {
            Phase::AutoPkgTest(n) => self.add_test_dependency(n, requirement),
            Phase::Build => self.add_build_dependency(requirement),
            Phase::BuildEnv => {
                // TODO(jelmer): Actually, we probably just want to install it on the host system?
                log::warn!("Unknown phase {:?}", phase);
                Ok(false)
            }
            Phase::CreateSession => {
                log::warn!("Unknown phase {:?}", phase);
                Ok(false)
            }
        }
    }

    /// Create an editor for the debian/control file.
    ///
    /// # Returns
    /// An editor for the control file, or Error if not found or cannot be edited
    pub fn edit_control<'a>(&'a self) -> Result<Box<dyn AbstractControlEditor + 'a>, Error> {
        if self
            .tree
            .has_filename(&self.subpath.join("debian/debcargo.toml"))
        {
            Ok(Box::new(
                debian_analyzer::debcargo::DebcargoEditor::from_directory(
                    &self.tree.abspath(&self.subpath).unwrap(),
                )?,
            ))
        } else {
            let control_path = Path::new("debian/control");
            Ok(
                Box::new(self.edit_file::<debian_control::Control>(control_path, false)?)
                    as Box<dyn AbstractControlEditor>,
            )
        }
    }

    fn add_build_dependency(&self, requirement: &DebianDependency) -> Result<bool, Error> {
        assert!(!requirement.is_empty());
        let mut control = self.edit_control()?;

        for binary in control.binaries() {
            if requirement.touches_package(&binary.name().unwrap()) {
                return Err(Error::CircularDependency(binary.name().unwrap()));
            }
        }

        let mut source = if let Some(source) = control.source() {
            source
        } else {
            return Err(Error::MissingSource);
        };
        for rel in requirement.iter() {
            source.ensure_build_dep(rel);
        }

        std::mem::drop(source);

        let desc = requirement.relation_string();

        if !control.commit() {
            log::info!("Giving up; build dependency {} was already present.", desc);
            return Ok(false);
        }

        log::info!("Adding build dependency: {}", desc);
        self.commit(&format!("Add missing build dependency on {}.", desc), None)?;
        Ok(true)
    }

    /// Create an editor for the debian/tests/control file.
    ///
    /// # Returns
    /// An editor for the tests control file, or Error if not found or cannot be edited
    pub fn edit_tests_control(&self) -> Result<TreeEditor<'_, deb822_lossless::Deb822>, Error> {
        Ok(self.edit_file::<deb822_lossless::Deb822>(Path::new("debian/tests/control"), false)?)
    }

    /// Create an editor for the debian/rules file.
    ///
    /// # Returns
    /// An editor for the rules file, or Error if not found or cannot be edited
    pub fn edit_rules(&self) -> Result<TreeEditor<'_, makefile_lossless::Makefile>, Error> {
        Ok(self.edit_file::<makefile_lossless::Makefile>(Path::new("debian/rules"), false)?)
    }

    fn add_test_dependency(
        &self,
        testname: &str,
        requirement: &DebianDependency,
    ) -> Result<bool, Error> {
        // TODO(jelmer): If requirement is for one of our binary packages  but "@" is already
        // present then don't do anything.

        let editor = self.edit_tests_control()?;

        let mut command_counter = 1;
        for mut para in editor.paragraphs() {
            let name = para.get("Tests").unwrap_or_else(|| {
                let name = format!("command{}", command_counter);
                command_counter += 1;
                name
            });

            if name != testname {
                continue;
            }

            for rel in requirement.iter() {
                let depends = para.get("Depends").unwrap_or_default();
                let mut rels: debian_control::lossless::relations::Relations =
                    depends.parse().map_err(|e| {
                        Error::InvalidField(format!("Test Depends for {}", testname), e)
                    })?;
                debian_analyzer::relations::ensure_relation(&mut rels, rel);
                para.insert("Depends", &rels.to_string());
            }
        }

        let desc = requirement.relation_string();

        if editor.commit()?.is_empty() {
            log::info!(
                "Giving up; dependency {} for test {} was already present.",
                desc,
                testname,
            );
            return Ok(false);
        }

        log::info!("Adding dependency to test {}: {}", testname, desc);
        self.commit(
            &format!("Add missing dependency for test {} on {}.", testname, desc),
            None,
        )?;
        Ok(true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
    pub const COMMITTER: &str = "ognibuild <ognibuild@example.com>";
    fn setup(path: &Path) -> DebianPackagingContext {
        let tree = create_standalone_workingtree(path, &ControlDirFormat::default()).unwrap();
        std::fs::create_dir_all(path.join("debian")).unwrap();
        std::fs::write(
            path.join("debian/control"),
            r###"Source: blah
Build-Depends: libc6

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
"###,
        )
        .unwrap();
        std::fs::write(
            path.join("debian/changelog"),
            r###"blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 04 Apr 2020 14:12:13 +0000
"###,
        )
        .unwrap();
        tree.add(&[
            Path::new("debian"),
            Path::new("debian/control"),
            Path::new("debian/changelog"),
        ])
        .unwrap();
        tree.build_commit()
            .message("Initial commit")
            .committer(COMMITTER)
            .commit()
            .unwrap();

        DebianPackagingContext::new(
            tree,
            Path::new(""),
            Some(("ognibuild".to_owned(), "<ognibuild@jelmer.uk>".to_owned())),
            false,
            Some(Box::new(breezyshim::commit::NullCommitReporter::new())),
        )
    }

    #[test]
    fn test_already_present() {
        let td = tempfile::tempdir().unwrap();
        let context = setup(td.path());
        let dep = DebianDependency::simple("libc6");
        assert!(!context.add_build_dependency(&dep).unwrap());
    }

    #[test]
    fn test_basic() {
        let td = tempfile::tempdir().unwrap();
        let context = setup(td.path());
        let dep = DebianDependency::simple("foo");
        assert!(context.add_build_dependency(&dep).unwrap());
        let control = std::fs::read_to_string(td.path().join("debian/control")).unwrap();
        assert_eq!(
            control,
            r###"Source: blah
Build-Depends: foo, libc6

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
"###
        );
    }

    #[test]
    fn test_circular() {
        let td = tempfile::tempdir().unwrap();
        let context = setup(td.path());
        let dep = DebianDependency::simple("python-blah");
        assert!(matches!(
            context.add_build_dependency(&dep),
            Err(Error::CircularDependency(_))
        ));
    }
}
