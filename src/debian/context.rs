use crate::dependencies::debian::DebianDependency;
use breezyshim::commit::CommitReporter;
use breezyshim::debian::debcommit::debcommit;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
pub use buildlog_consultant::sbuild::Phase;
use debian_analyzer::abstract_control::AbstractControlEditor;
use debian_analyzer::editor::{Editor, EditorError, Marshallable, MutableTreeEdit, TreeEditor};
use std::path::{Path, PathBuf};

#[derive(Debug)]
pub enum Error {
    CircularDependency(String),
    /// No source stanza
    MissingSource,
    BrzError(BrzError),
    EditorError(debian_analyzer::editor::EditorError),
    IoError(std::io::Error),
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

pub struct DebianPackagingContext {
    pub tree: WorkingTree,
    pub subpath: PathBuf,
    pub committer: (String, String),
    pub update_changelog: bool,
    pub commit_reporter: Option<Box<dyn CommitReporter>>,
}

impl DebianPackagingContext {
    pub fn new(
        tree: WorkingTree,
        subpath: &Path,
        committer: Option<(String, String)>,
        update_changelog: bool,
        commit_reporter: Option<Box<dyn CommitReporter>>,
    ) -> Self {
        Self {
            tree,
            subpath: subpath.to_path_buf(),
            committer: committer.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap()),
            update_changelog,
            commit_reporter,
        }
    }

    pub fn has_filename(&self, path: &Path) -> bool {
        self.tree.has_filename(&self.subpath.join(path))
    }

    pub fn abspath(&self, path: &Path) -> PathBuf {
        self.tree.abspath(&self.subpath.join(path)).unwrap()
    }

    pub fn edit_file<P: Marshallable>(
        &self,
        path: &std::path::Path,
        allow_generated: bool,
    ) -> Result<TreeEditor<P>, EditorError> {
        let path = self.subpath.join(path);
        self.tree.edit_file(&path, allow_generated, true)
    }

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
            let mut builder = self.tree
                .build_commit()
                .message(summary)
                .committer(&committer)
                .specific_files(&[&self.subpath]);
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

    pub fn edit_tests_control(&self) -> Result<TreeEditor<deb822_lossless::Deb822>, Error> {
        Ok(self.edit_file::<deb822_lossless::Deb822>(Path::new("debian/tests/control"), false)?)
    }

    pub fn edit_rules(&self) -> Result<TreeEditor<makefile_lossless::Makefile>, Error> {
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
Build-Depends: libc6, foo

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
