use crate::dependencies::debian::DebianDependency;
use breezyshim::commit::CommitReporter;
use breezyshim::debian::debcommit::debcommit;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
use buildlog_consultant::sbuild::Phase;
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

pub struct DebianPackagingContext {
    tree: WorkingTree,
    subpath: PathBuf,
    committer: (String, String),
    update_changelog: bool,
    commit_reporter: Box<dyn CommitReporter>,
}

impl DebianPackagingContext {
    pub fn new(
        tree: WorkingTree,
        subpath: PathBuf,
        committer: Option<(String, String)>,
        update_changelog: bool,
        commit_reporter: Box<dyn CommitReporter>,
    ) -> Self {
        Self {
            tree,
            subpath,
            committer: committer.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap()),
            update_changelog,
            commit_reporter,
        }
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
        self.tree.edit_file(&path, false, allow_generated)
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
                Some(self.commit_reporter.as_ref()),
                None,
            )
        } else {
            self.tree
                .build_commit()
                .message(summary)
                .committer(&committer)
                .specific_files(&[&self.subpath])
                .reporter(self.commit_reporter.as_ref())
                .commit()
        };

        std::mem::drop(lock_write);

        match r {
            Ok(_) => Ok(true),
            Err(BrzError::PointlessCommit) => Ok(false),
            Err(e) => {
                return Err(e.into());
            }
        }
    }

    pub fn add_dependency(
        &self,
        phase: &Phase,
        requirement: &DebianDependency,
    ) -> Result<bool, Error> {
        match phase {
            Phase::AutoPkgTest(n) => return self.add_test_dependency(n, requirement),
            Phase::Build => return self.add_build_dependency(requirement),
            Phase::BuildEnv => {
                // TODO(jelmer): Actually, we probably just want to install it on the host system?
                log::warn!("Unknown phase {:?}", phase);
                return Ok(false);
            }
            Phase::CreateSession => {
                log::warn!("Unknown phase {:?}", phase);
                return Ok(false);
            }
        }
    }

    fn add_build_dependency(&self, requirement: &DebianDependency) -> Result<bool, Error> {
        let mut control: Box<dyn AbstractControlEditor> = if self
            .tree
            .has_filename(&self.subpath.join("debian/debcargo.toml"))
        {
            Box::new(debian_analyzer::debcargo::DebcargoEditor::from_directory(
                &self.tree.abspath(&self.subpath)?,
            )?)
        } else {
            let control_path = self.abspath(Path::new("debian/control"));
            Box::new(self.edit_file::<debian_control::Control>(&control_path, false)?)
                as Box<dyn AbstractControlEditor>
        };

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

        if control.commit() {
            log::info!("Giving up; build dependency {} was already present.", desc);
            return Ok(false);
        }

        log::info!("Adding build dependency: {}", desc);
        self.commit(&format!("Add missing build dependency on {}.", desc), None)?;
        Ok(true)
    }

    fn add_test_dependency(
        &self,
        testname: &str,
        requirement: &DebianDependency,
    ) -> Result<bool, Error> {
        // TODO(jelmer): If requirement is for one of our binary packages  but "@" is already
        // present then don't do anything.

        let editor =
            self.edit_file::<deb822_lossless::Deb822>(Path::new("debian/tests/control"), false)?;

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
                let mut rels: debian_control::relations::Relations =
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
