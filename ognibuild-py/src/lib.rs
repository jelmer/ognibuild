use pyo3::import_exception;
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

#[pyfunction]
pub fn dupe_vcs_tree(py: Python, tree: PyObject, directory: std::path::PathBuf) -> PyResult<()> {
    if tree.bind(py).hasattr("_repository")? {
        let tree = breezyshim::tree::RevisionTree(tree);
        ognibuild::vcs::dupe_vcs_tree(&tree, &directory)
    } else {
        let tree = breezyshim::tree::WorkingTree(tree);
        ognibuild::vcs::dupe_vcs_tree(&tree, &directory)
    }
    .map_err(|e| e.into())
}

struct PyProblem(PyObject);

impl PartialEq for PyProblem {
    fn eq(&self, other: &Self) -> bool {
        pyo3::Python::with_gil(|py| {
            let eq = self.0.getattr(py, "__eq__")?;
            eq.call1(py, (other.0.clone_ref(py),))?.extract(py)
        })
        .unwrap_or(false)
    }
}

impl Eq for PyProblem {}

impl std::hash::Hash for PyProblem {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        pyo3::Python::with_gil(|py| {
            let hash = self.0.getattr(py, "__hash__")?;
            hash.call0(py)?.extract(py)
        })
        .unwrap_or(0)
        .hash(state)
    }
}

impl IntoPy<PyObject> for PyProblem {
    fn into_py(self, py: Python) -> PyObject {
        self.0.into_py(py)
    }
}

impl std::fmt::Display for PyProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PyProblem")
    }
}

impl std::fmt::Debug for PyProblem {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PyProblem").field(&self.0).finish()
    }
}

struct PyBuildFixer(PyObject);

impl std::fmt::Debug for PyBuildFixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_tuple("PyBuildFixer").field(&self.0).finish()
    }
}

impl std::fmt::Display for PyBuildFixer {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("PyBuildFixer")
    }
}

impl ognibuild::fix_build::BuildFixer<PyErr, PyProblem> for PyBuildFixer {
    fn can_fix(&self, problem: &PyProblem) -> bool {
        pyo3::Python::with_gil(|py| {
            let can_fix = self.0.getattr(py, "can_fix")?;
            can_fix.call1(py, (problem.0.clone_ref(py),))?.extract(py)
        })
        .unwrap_or(false)
    }

    fn fix(
        &self,
        problem: &PyProblem,
        phase: &[&str],
    ) -> Result<bool, ognibuild::fix_build::Error<PyErr, PyProblem>> {
        pyo3::Python::with_gil(|py| {
            let fix = self.0.getattr(py, "fix")?;
            fix.call1(py, (problem.0.clone_ref(py), phase.to_vec()))?
                .extract(py)
        })
        .map_err(ognibuild::fix_build::Error::Other)
    }
}

#[pyfunction]
fn iterate_with_build_fixers(
    fixers: Vec<PyObject>,
    phase: Vec<String>,
    cb: PyObject,
    limit: Option<usize>,
) -> Result<PyObject, PyErr> {
    let fixers = fixers
        .into_iter()
        .map(|e| Box::new(PyBuildFixer(e)))
        .collect::<Vec<_>>();
    let cb = || -> Result<_, ognibuild::fix_build::Error<PyErr, PyProblem>> {
        pyo3::Python::with_gil(|py| cb.call0(py).map_err(ognibuild::fix_build::Error::Other))
    };
    ognibuild::fix_build::iterate_with_build_fixers(
        fixers
            .iter()
            .map(|p| p.as_ref() as &dyn ognibuild::fix_build::BuildFixer<PyErr, PyProblem>)
            .collect::<Vec<_>>()
            .as_slice(),
        phase
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
        cb,
        limit,
    )
    .map_err(|e| match e {
        ognibuild::fix_build::IterateBuildError::Other(e) => e,
        ognibuild::fix_build::IterateBuildError::FixerLimitReached(limit) => {
            import_exception!(silver_platter.fix_build, FixerLimitReached);
            PyErr::new::<FixerLimitReached, _>((limit,))
        }
        ognibuild::fix_build::IterateBuildError::PersistentBuildProblem(problem) => {
            import_exception!(silver_platter.fix_build, PersistentBuildProblem);
            PyErr::new::<PersistentBuildProblem, _>((problem,))
        }
    })
}

#[pyfunction]
fn resolve_error(
    py: Python,
    problem: PyObject,
    phase: Vec<String>,
    fixers: Vec<PyObject>,
) -> PyResult<bool> {
    let phase = phase.as_slice();
    let problem = PyProblem(problem);
    let fixers = fixers.into_iter().map(PyBuildFixer).collect::<Vec<_>>();
    let r = ognibuild::fix_build::resolve_error(
        &problem,
        phase
            .iter()
            .map(|x| x.as_str())
            .collect::<Vec<_>>()
            .as_slice(),
        fixers
            .iter()
            .map(|p| p as &dyn ognibuild::fix_build::BuildFixer<PyErr, PyProblem>)
            .collect::<Vec<_>>()
            .as_slice(),
    );
    match r {
        Ok(r) => Ok(r),
        Err(e) => Err(match e {
            ognibuild::fix_build::Error::Other(e) => e,
            ognibuild::fix_build::Error::BuildProblem(problem) => {
                PyErr::from_value(problem.0.as_ref(py))
            }
        }),
    }
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &PyModule) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(sanitize_session_name))?;
    m.add_wrapped(wrap_pyfunction!(generate_session_id))?;
    m.add_wrapped(wrap_pyfunction!(export_vcs_tree))?;
    m.add_wrapped(wrap_pyfunction!(dupe_vcs_tree))?;
    m.add_wrapped(wrap_pyfunction!(iterate_with_build_fixers))?;
    m.add_wrapped(wrap_pyfunction!(resolve_error))?;
    Ok(())
}
