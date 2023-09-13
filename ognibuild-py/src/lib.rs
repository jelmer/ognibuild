use pyo3::prelude::*;

#[pyfunction]
fn sanitize_session_name(name: &str) -> String {
    ognibuild::sanitize_session_name(name)
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(sanitize_session_name))?;
    Ok(())
}
