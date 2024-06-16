use breezyshim::tree::Tree;
use pyo3::exceptions::PyIOError;
use pyo3::import_exception;
use pyo3::prelude::*;
use std::path::Path;
use url::Url;

import_exception!(ognibuild, DetailedFailure);

pub fn export_vcs_tree(
    tree: &dyn Tree,
    directory: &Path,
    subpath: Option<&Path>,
) -> Result<(), PyErr> {
    Python::with_gil(|py| {
        match breezyshim::export::export(tree, directory, subpath) {
            Ok(_) => {}
            Err(e) => {
                if e.is_instance_of::<PyIOError>(py) {
                    let e: std::io::Error = e.into();
                    let m = py.import_bound("buildlog_consultant.common").unwrap();
                    let no_space_on_device_cls = m.getattr("NoSpaceOnDevice").unwrap();
                    let no_space_on_device = no_space_on_device_cls.call0().unwrap().to_object(py);

                    if e.raw_os_error() == Some(libc::ENOSPC) {
                        return Err(DetailedFailure::new_err((
                            1,
                            vec!["export"],
                            no_space_on_device,
                        )));
                    } else {
                        panic!("Unexpected error: {:?}", e);
                    }
                } else {
                    panic!("Unexpected error: {:?}", e);
                }
            }
        }
        Ok(())
    })
}

pub trait DupableTree {
    fn tree(&self) -> breezyshim::tree::RevisionTree;

    fn get_parent(&self) -> Option<String>;
}

impl DupableTree for &breezyshim::tree::WorkingTree {
    fn tree(&self) -> breezyshim::tree::RevisionTree {
        self.basis_tree()
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
) -> Result<(), breezyshim::controldir::OpenError> {
    let tree = orig_tree.tree();
    let result = tree.repository().controldir().sprout(
        Url::from_directory_path(directory).unwrap(),
        None,
        Some(true),
        None,
        Some(&tree.get_revision_id()),
    );

    assert!(result.has_workingtree());

    // Copy parent location - some scripts need this
    if let Some(parent) = orig_tree.get_parent() {
        let mut branch = result.open_branch(None).unwrap();
        branch.set_parent(&parent);
    }

    Ok(())
}
