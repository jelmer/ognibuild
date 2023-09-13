use pyo3::prelude::*;

#[pyfunction]
fn sanitize_session_name(name: &str) -> String {
    ognibuild::session::schroot::sanitize_session_name(name)
}

#[pyfunction]
fn generate_session_id(name: &str) -> String {
    ognibuild::session::schroot::generate_session_id(name)
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(sanitize_session_name))?;
    m.add_wrapped(wrap_pyfunction!(generate_session_id))?;
    Ok(())
}
