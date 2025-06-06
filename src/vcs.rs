//! VCS-related functions
use breezyshim::branch::Branch;
use breezyshim::error::Error as BrzError;
use breezyshim::prelude::Repository;
use breezyshim::tree::PyTree;
use breezyshim::workingtree::{GenericWorkingTree, WorkingTree};
use std::path::{Path, PathBuf};
use url::Url;

/// Export a VCS tree to a new location.
///
/// # Arguments
/// * `tree` - The tree to export
/// * `directory` - The directory to export the tree to
/// * `subpath` - The subpath to export
pub fn export_vcs_tree<T: PyTree>(
    tree: &T,
    directory: &Path,
    subpath: Option<&Path>,
) -> Result<(), BrzError> {
    breezyshim::export::export(tree, directory, subpath)
}

/// A Breezy tree that can be duplicated.
pub trait DupableTree {
    /// Get the basis tree of this tree.
    fn basis_tree(&self) -> breezyshim::tree::RevisionTree;

    /// Get the parent location of this tree.
    fn get_parent(&self) -> Option<String>;

    /// Get the base directory of this tree, if it has one.
    fn basedir(&self) -> Option<PathBuf>;

    /// Export this tree to a directory.
    fn export_to(&self, directory: &Path, subpath: Option<&Path>) -> Result<(), BrzError>;
}

impl DupableTree for GenericWorkingTree {
    fn basis_tree(&self) -> breezyshim::tree::RevisionTree {
        WorkingTree::basis_tree(self).unwrap()
    }

    fn get_parent(&self) -> Option<String> {
        WorkingTree::branch(self).get_parent()
    }

    fn basedir(&self) -> Option<PathBuf> {
        Some(WorkingTree::basedir(self))
    }

    fn export_to(&self, directory: &Path, subpath: Option<&Path>) -> Result<(), BrzError> {
        export_vcs_tree(self, directory, subpath)
    }
}

impl DupableTree for breezyshim::tree::RevisionTree {
    fn basis_tree(&self) -> breezyshim::tree::RevisionTree {
        self.repository()
            .revision_tree(&self.get_revision_id())
            .unwrap()
    }

    fn get_parent(&self) -> Option<String> {
        let branch = self.repository().controldir().open_branch(None).unwrap();

        branch.get_parent()
    }

    fn basedir(&self) -> Option<PathBuf> {
        None
    }

    fn export_to(&self, directory: &Path, subpath: Option<&Path>) -> Result<(), BrzError> {
        export_vcs_tree(self, directory, subpath)
    }
}

/// Duplicate a VCS tree to a new location, including all history.
///
/// For a RevisionTree, this will duplicate the tree to a new location.
/// For a WorkingTree, this will duplicate the basis tree to a new location.
///
/// # Arguments
/// * `orig_tree` - The tree to duplicate
/// * `directory` - The directory to duplicate the tree to
pub fn dupe_vcs_tree(orig_tree: &dyn DupableTree, directory: &Path) -> Result<(), BrzError> {
    let tree = orig_tree.basis_tree();
    let result = tree.repository().controldir().sprout(
        Url::from_directory_path(directory).unwrap(),
        None,
        Some(true),
        None,
        Some(&tree.get_revision_id()),
    )?;

    assert!(result.has_workingtree());

    // Copy parent location - some scripts need this
    if let Some(parent) = orig_tree.get_parent() {
        let mut branch = result.open_branch(None)?;
        branch.set_parent(&parent);
    }

    Ok(())
}
