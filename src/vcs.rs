use breezyshim::tree::Tree;
use breezyshim::error::Error as BrzError;
use std::path::{PathBuf, Path};
use url::Url;

/// Export a VCS tree to a new location.
///
/// # Arguments
/// * `tree` - The tree to export
/// * `directory` - The directory to export the tree to
/// * `subpath` - The subpath to export
pub fn export_vcs_tree(
    tree: &dyn Tree,
    directory: &Path,
    subpath: Option<&Path>,
) -> Result<(), BrzError> {
    breezyshim::export::export(tree, directory, subpath)
}

pub trait DupableTree {
    fn basis_tree(&self) -> breezyshim::tree::RevisionTree;

    fn get_parent(&self) -> Option<String>;

    fn basedir(&self) -> Option<PathBuf>;

    fn as_tree(&self) -> &dyn Tree;
}

impl DupableTree for breezyshim::workingtree::WorkingTree {
    fn basis_tree(&self) -> breezyshim::tree::RevisionTree {
        self.basis_tree().unwrap()
    }

    fn get_parent(&self) -> Option<String> {
        self.branch().get_parent()
    }

    fn basedir(&self) -> Option<PathBuf> {
        Some(self.basedir())
    }

    fn as_tree(&self) -> &dyn Tree {
        self
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

    fn as_tree(&self) -> &dyn Tree {
        self
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
pub fn dupe_vcs_tree(
    orig_tree: &dyn DupableTree,
    directory: &Path,
) -> Result<(), BrzError> {
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
