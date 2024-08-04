use breezyshim::tree::Tree;
use breezyshim::error::Error as BrzError;
use std::path::Path;
use url::Url;

pub fn export_vcs_tree(
    tree: &dyn Tree,
    directory: &Path,
    subpath: Option<&Path>,
) -> Result<(), BrzError> {
    breezyshim::export::export(tree, directory, subpath)
}

pub trait DupableTree {
    fn tree(&self) -> breezyshim::tree::RevisionTree;

    fn get_parent(&self) -> Option<String>;
}

impl DupableTree for &breezyshim::tree::WorkingTree {
    fn tree(&self) -> breezyshim::tree::RevisionTree {
        self.basis_tree().unwrap()
    }

    fn get_parent(&self) -> Option<String> {
        self.branch().get_parent()
    }
}

impl DupableTree for &breezyshim::tree::RevisionTree {
    fn tree(&self) -> breezyshim::tree::RevisionTree {
        self.repository()
            .revision_tree(&self.get_revision_id())
            .unwrap()
    }

    fn get_parent(&self) -> Option<String> {
        let branch = self.repository().controldir().open_branch(None).unwrap();

        branch.get_parent()
    }
}

pub fn dupe_vcs_tree(
    orig_tree: impl DupableTree,
    directory: &Path,
) -> Result<(), BrzError> {
    let tree = orig_tree.tree();
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
