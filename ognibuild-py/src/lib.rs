use pyo3::prelude::*;

#[pyfunction]
fn sanitize_session_name(name: &str) -> String {
    ognibuild::session::schroot::sanitize_session_name(name)
}

#[pyfunction]
fn generate_session_id(name: &str) -> String {
    ognibuild::session::schroot::generate_session_id(name)
}

#[pyfunction]
pub fn export_vcs_tree(
    tree: PyObject,
    directory: std::path::PathBuf,
    subpath: Option<std::path::PathBuf>,
) -> Result<(), PyErr> {
    let tree = breezyshim::tree::RevisionTree(tree);
    ognibuild::vcs::export_vcs_tree(&tree, &directory, subpath.as_deref())
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(sanitize_session_name))?;
    m.add_wrapped(wrap_pyfunction!(generate_session_id))?;
    m.add_wrapped(wrap_pyfunction!(export_vcs_tree))?;
    Ok(())
}
