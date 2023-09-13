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
    tree: &dyn breezyshim::tree::Tree,
    directory: &std::path::Path,
    subpath: Option<&std::path::Path>,
) -> Result<(), PyErr> {
    ognibuild::vcs::export_vcs_tree(tree, directory, subpath)
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(sanitize_session_name))?;
    m.add_wrapped(wrap_pyfunction!(generate_session_id))?;
    m.add_wrapped(wrap_pyfunction!(export_vcs_tree))?;
    Ok(())
}
