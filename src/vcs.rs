use breezyshim::tree::Tree;
use pyo3::exceptions::PyIOError;
use pyo3::import_exception;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::path::Path;

import_exception!(ognibuild, DetailedFailure);

pub fn export_vcs_tree(
    tree: &dyn Tree,
    directory: &Path,
    subpath: Option<&Path>,
) -> Result<(), PyErr> {
    let subpath = subpath.unwrap_or_else(|| Path::new(""));
    Python::with_gil(|py| {
        let m = py.import("breezy.export").unwrap();
        let export = m.getattr("export").unwrap();
        let kwargs = PyDict::new(py);
        let subpath = if subpath == Path::new("") {
            None
        } else {
            Some(subpath)
        };
        kwargs.set_item("subdir", subpath).unwrap();
        match export.call(
            (tree.to_object(py), directory, "dir", py.None()),
            Some(kwargs),
        ) {
            Ok(_) => {}
            Err(e) => {
                if e.is_instance_of::<PyIOError>(py) {
                    let e: std::io::Error = e.into();
                    let m = py.import("buildlog_consultant.common").unwrap();
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
