use pyo3::exceptions::PyException;
use pyo3::import_exception;
use pyo3::prelude::*;
use std::collections::HashMap;
use std::io::{Read, Write};

pyo3::create_exception!(
    ognibuild.session,
    NoSessionOpen,
    pyo3::exceptions::PyException
);
pyo3::create_exception!(
    ognibuild.session,
    SessionAlreadyOpen,
    pyo3::exceptions::PyException
);
pyo3::create_exception!(
    ognibuild.dist_catcher,
    DistNoTarball,
    pyo3::exceptions::PyException
);

#[pyclass(extends=PyException)]
struct SessionSetupFailure {
    #[pyo3(get)]
    errlines: Vec<String>,
    #[pyo3(get)]
    reason: String,
}

#[pymethods]
impl SessionSetupFailure {
    #[new]
    fn new(errlines: Vec<String>, reason: String) -> Self {
        SessionSetupFailure { errlines, reason }
    }
}

impl From<SessionSetupFailure> for PyErr {
    fn from(e: SessionSetupFailure) -> PyErr {
        Self::new::<SessionSetupFailure, _>((e.errlines, e.reason))
    }
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
#[pyo3(signature = (fixers, phase, cb, limit=None))]
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
                PyErr::from_value_bound(problem.0.into_bound(py))
            }
        }),
    }
}

#[pyfunction]
fn shebang_binary(path: &str) -> PyResult<Option<String>> {
    ognibuild::shebang_binary(std::path::Path::new(path)).map_err(|e| e.into())
}

pyo3::import_exception!(subprocess, CalledProcessError);

#[pyclass(subclass)]
struct Session(Option<std::sync::Mutex<Box<dyn ognibuild::session::Session + Send>>>);

fn map_session_error(e: ognibuild::session::Error) -> PyErr {
    match e {
        ognibuild::session::Error::CalledProcessError(e) => CalledProcessError::new_err(e),
        ognibuild::session::Error::SetupFailure(n, e) => {
            SessionSetupFailure::new(vec![n], e).into()
        }
        ognibuild::session::Error::IoError(e) => e.into(),
    }
}

impl Session {
    fn get_session(
        &self,
    ) -> PyResult<std::sync::MutexGuard<Box<dyn ognibuild::session::Session + Send>>> {
        if let Some(ref s) = self.0 {
            Ok(s.lock().unwrap())
        } else {
            Err(NoSessionOpen::new_err(()))
        }
    }
}

#[pymethods]
impl Session {
    fn create_home(&self) -> PyResult<()> {
        self.get_session()?.create_home().map_err(map_session_error)
    }

    #[pyo3(signature = (path))]
    fn chdir(&self, path: std::path::PathBuf) -> PyResult<()> {
        self.get_session()?
            .chdir(path.as_path())
            .map_err(map_session_error)
    }

    #[getter]
    fn location(&self) -> PyResult<std::path::PathBuf> {
        Ok(self.get_session()?.location())
    }

    #[pyo3(signature = (path))]
    fn external_path(&self, path: std::path::PathBuf) -> PyResult<std::path::PathBuf> {
        Ok(self.get_session()?.external_path(path.as_path()))
    }

    #[pyo3(signature = (argv, cwd=None, user=None, env=None))]
    fn check_call(
        &self,
        argv: Vec<String>,
        cwd: Option<std::path::PathBuf>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<()> {
        let argv = argv.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        self.get_session()?
            .check_call(argv, cwd.as_deref(), user, env)
            .map_err(map_session_error)
    }

    #[pyo3(signature = (argv, cwd=None, user=None, env=None))]
    fn check_output(
        &self,
        py: Python,
        argv: Vec<String>,
        cwd: Option<std::path::PathBuf>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<PyObject> {
        let argv = argv.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        self.get_session()?
            .check_output(argv, cwd.as_deref(), user, env)
            .map_err(map_session_error)
            .map(|x| pyo3::types::PyBytes::new_bound(py, &x).into())
    }

    #[pyo3(signature = (path))]
    fn exists(&self, path: std::path::PathBuf) -> PyResult<bool> {
        Ok(self.get_session()?.exists(path.as_path()))
    }

    #[pyo3(signature = (path))]
    fn mkdir(&self, path: std::path::PathBuf) -> PyResult<()> {
        self.get_session()?
            .mkdir(path.as_path())
            .map_err(map_session_error)
    }

    #[pyo3(signature = (path))]
    fn rmtree(&self, path: std::path::PathBuf) -> PyResult<()> {
        self.get_session()?
            .rmtree(path.as_path())
            .map_err(map_session_error)
    }

    #[pyo3(signature = (path, subdir=None))]
    fn setup_from_directory(
        &self,
        path: std::path::PathBuf,
        subdir: Option<&str>,
    ) -> PyResult<(std::path::PathBuf, std::path::PathBuf)> {
        self.get_session()?
            .setup_from_directory(path.as_path(), subdir)
            .map_err(map_session_error)
    }

    #[pyo3(signature = (tree, include_controldir=None, subdir=None))]
    fn setup_from_vcs(
        &self,
        py: Python,
        tree: PyObject,
        include_controldir: Option<bool>,
        subdir: Option<std::path::PathBuf>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), PyErr> {
        let tree: Box<dyn ognibuild::vcs::DupableTree> = if tree.bind(py).hasattr("_repository")? {
            Box::new(breezyshim::tree::RevisionTree(tree)) as _
        } else {
            Box::new(breezyshim::tree::WorkingTree::from(tree)) as _
        };
        self.get_session()?
            .setup_from_vcs(tree.as_ref(), include_controldir, subdir.as_deref())
            .map_err(map_session_error)
    }

    #[getter]
    fn is_temporary(&self) -> PyResult<bool> {
        Ok(self.get_session()?.is_temporary())
    }

    #[allow(non_snake_case)]
    #[pyo3(signature = (argv, cwd=None, user=None, stdout=None, stderr=None, stdin=None, env=None))]
    #[allow(clippy::too_many_arguments)]
    fn Popen(
        &self,
        argv: Vec<String>,
        cwd: Option<std::path::PathBuf>,
        user: Option<&str>,
        stdout: Option<PyObject>,
        stderr: Option<PyObject>,
        stdin: Option<PyObject>,
        env: Option<HashMap<String, String>>,
    ) -> PyResult<Child> {
        let argv = argv.iter().map(|x| x.as_str()).collect::<Vec<_>>();
        let stdout = extract_stdio(stdout)?;
        let stderr = extract_stdio(stderr)?;
        let stdin = extract_stdio(stdin)?;
        let child =
            self.get_session()?
                .popen(argv, cwd.as_deref(), user, stdout, stderr, stdin, env);
        Ok(Child::from(child))
    }
}

fn extract_stdio(o: Option<PyObject>) -> PyResult<Option<std::process::Stdio>> {
    fn py_eq(a: &Bound<PyAny>, b: &Bound<PyAny>) -> PyResult<bool> {
        a.call_method1("__eq__", (b,))?.extract()
    }
    if o.is_none() {
        return Ok(None);
    }
    let o = o.unwrap();
    let p = Python::with_gil(|py| -> PyResult<_> {
        let m = py.import_bound("subprocess")?;
        let pipe = m.getattr("PIPE")?;
        let devnull = m.getattr("DEVNULL")?;
        let stdout = m.getattr("STDOUT")?;
        if py_eq(o.bind(py), &pipe)? {
            Ok(std::process::Stdio::piped())
        } else if py_eq(o.bind(py), &devnull)? {
            Ok(std::process::Stdio::null())
        } else if py_eq(o.bind(py), &stdout)? {
            Ok(std::process::Stdio::inherit())
        } else {
            let fd = o.call_method0(py, "fileno")?.extract::<i32>(py)?;
            use std::os::unix::io::FromRawFd;
            let f = unsafe { std::fs::File::from_raw_fd(fd) };
            Ok(std::process::Stdio::from(f))
        }
    })?;
    Ok(Some(p))
}

#[pyclass]
struct ChildStdin(std::process::ChildStdin);

#[pymethods]
impl ChildStdin {
    fn write(&mut self, data: &[u8]) -> PyResult<()> {
        Ok(self.0.write_all(data)?)
    }

    fn flush(&mut self) -> PyResult<()> {
        self.0.flush()?;
        Ok(())
    }
}

#[pyclass]
struct ChildStdout(std::process::ChildStdout);

#[pymethods]
impl ChildStdout {
    fn read(&mut self, size: usize) -> PyResult<Vec<u8>> {
        let mut buf = vec![0; size];
        let n = self.0.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }
}

#[pyclass]
struct ChildStderr(std::process::ChildStderr);

#[pymethods]
impl ChildStderr {
    fn read(&mut self, size: usize) -> PyResult<Vec<u8>> {
        let mut buf = vec![0; size];
        let n = self.0.read(&mut buf)?;
        buf.truncate(n);
        Ok(buf)
    }
}

#[pyclass]
struct Child {
    child: std::process::Child,
}

impl From<std::process::Child> for Child {
    fn from(child: std::process::Child) -> Self {
        Child { child }
    }
}

#[pymethods]
impl Child {
    #[getter]
    fn returncode(&mut self) -> PyResult<Option<i32>> {
        Ok(self.child.try_wait()?.and_then(|x| x.code()))
    }

    fn poll(&mut self) -> PyResult<Option<i32>> {
        Ok(self.child.try_wait()?.and_then(|x| x.code()))
    }

    fn terminate(&mut self) -> PyResult<()> {
        self.child.kill().map_err(|e| e.into())
    }

    fn wait(&mut self) -> PyResult<i32> {
        self.child
            .wait()
            .map(|x| x.code().unwrap_or(0))
            .map_err(|e| e.into())
    }

    // TODO: Add support for stdin, stdout, stderr
}

#[pyclass(extends=Session)]
struct PlainSession;

#[pymethods]
impl PlainSession {
    #[new]
    fn new() -> (Self, Session) {
        (PlainSession, Session(None))
    }

    fn __enter__<'p>(mut slf: PyRefMut<'p, Self>, _py: Python<'p>) -> PyResult<PyRefMut<'p, Self>> {
        if slf.as_super().0.is_some() {
            return Err(SessionAlreadyOpen::new_err(()));
        }
        let session =
            std::sync::Mutex::new(Box::new(ognibuild::session::plain::PlainSession::new()) as _);
        let sup = slf.as_super();
        sup.0 = Some(session);
        Ok(slf)
    }

    #[pyo3(signature = (exc_type, exc_value, traceback))]
    #[allow(unused_variables)]
    fn __exit__<'p>(
        mut slf: PyRefMut<'p, Self>,
        _py: Python<'p>,
        exc_type: Option<PyObject>,
        exc_value: Option<PyObject>,
        traceback: Option<PyObject>,
    ) -> PyResult<bool> {
        slf.as_super().0 = None;
        Ok(false)
    }
}

#[cfg(target_os = "linux")]
#[pyclass(extends=Session)]
struct SchrootSession {
    chroot: String,
    session_prefix: Option<String>,
}

#[cfg(target_os = "linux")]
#[pymethods]
impl SchrootSession {
    #[new]
    #[pyo3(signature = (chroot, session_prefix = None))]
    fn new(chroot: &str, session_prefix: Option<&str>) -> PyResult<(Self, Session)> {
        Ok((
            SchrootSession {
                chroot: chroot.to_string(),
                session_prefix: session_prefix.map(|x| x.to_string()),
            },
            Session(None),
        ))
    }

    #[pyo3(signature = ())]
    fn __enter__<'p>(mut slf: PyRefMut<'p, Self>, _py: Python<'p>) -> PyResult<PyRefMut<'p, Self>> {
        let session = std::sync::Mutex::new(Box::new(
            ognibuild::session::schroot::SchrootSession::new(
                &slf.chroot,
                slf.session_prefix.as_deref(),
            )
            .map_err(map_session_error)?,
        ) as _);
        let sup = slf.as_super();
        sup.0 = Some(session);
        Ok(slf)
    }

    #[pyo3(signature = (exc_type, exc_value, traceback))]
    #[allow(unused_variables)]
    fn __exit__<'p>(
        mut slf: PyRefMut<'p, Self>,
        _py: Python<'p>,
        exc_type: Option<PyObject>,
        exc_value: Option<PyObject>,
        traceback: Option<PyObject>,
    ) -> PyResult<bool> {
        slf.as_super().0 = None;
        Ok(false)
    }
}

#[pyfunction]
fn which(session: &Session, program: &str) -> PyResult<Option<std::path::PathBuf>> {
    Ok(ognibuild::session::which(session.get_session()?.as_ref(), program).map(|x| x.into()))
}

#[pyfunction]
fn get_user(session: &Session) -> PyResult<String> {
    Ok(ognibuild::session::get_user(session.get_session()?.as_ref()).to_string())
}

#[cfg(target_os = "linux")]
#[pyclass(extends=Session)]
struct UnshareSession;

#[pyfunction]
#[pyo3(signature = (session, args, cwd=None, user=None, env=None, stdin=None, stdout=None, stderr=None))]
fn run_with_tee(
    session: &Session,
    args: Vec<String>,
    cwd: Option<std::path::PathBuf>,
    user: Option<&str>,
    env: Option<HashMap<String, String>>,
    stdin: Option<PyObject>,
    stdout: Option<PyObject>,
    stderr: Option<PyObject>,
) -> PyResult<(i32, Vec<String>)> {
    let args = args.iter().map(|x| x.as_str()).collect::<Vec<_>>();
    let stdin = extract_stdio(stdin)?;
    let stdout = extract_stdio(stdout)?;
    let stderr = extract_stdio(stderr)?;
    let (ret, output) = ognibuild::session::run_with_tee(
        session.get_session()?.as_ref(),
        args,
        cwd.as_deref(),
        user,
        env,
        stdin,
        stdout,
        stderr,
    )
    .map_err(map_session_error)?;
    Ok((ret, output))
}

#[pyclass]
struct DistCatcher(ognibuild::dist_catcher::DistCatcher);

#[pymethods]
impl DistCatcher {
    #[new]
    #[pyo3(signature = (directories))]
    fn new(directories: Vec<String>) -> Self {
        DistCatcher(ognibuild::dist_catcher::DistCatcher::new(
            directories
                .into_iter()
                .map(std::path::PathBuf::from)
                .collect(),
        ))
    }

    #[staticmethod]
    #[pyo3(signature = (directory))]
    fn default(directory: &str) -> Self {
        DistCatcher(ognibuild::dist_catcher::DistCatcher::default(
            std::path::Path::new(directory),
        ))
    }

    #[pyo3(signature = ())]
    fn __enter__<'a>(mut slf: PyRefMut<'a, Self>) -> PyResult<PyRefMut<'a, Self>> {
        slf.0.start();
        Ok(slf)
    }

    #[pyo3(signature = (exc_type, exc_value, traceback))]
    #[allow(unused_variables)]
    fn __exit__(
        mut slf: PyRefMut<Self>,
        exc_type: Option<PyObject>,
        exc_value: Option<PyObject>,
        traceback: Option<PyObject>,
    ) -> PyResult<bool> {
        slf.0.find_files();
        Ok(false)
    }

    fn find_files(&mut self) -> Option<std::path::PathBuf> {
        self.0.find_files()
    }

    #[pyo3(signature = (path))]
    fn copy_single(&self, path: &str) -> PyResult<String> {
        if let Some(n) = self.0.copy_single(std::path::Path::new(path))? {
            Ok(n.to_string_lossy().to_string())
        } else {
            Err(DistNoTarball::new_err(()))
        }
    }
}

#[pymodule]
fn _ognibuild_rs(py: Python, m: &Bound<PyModule>) -> PyResult<()> {
    m.add_wrapped(wrap_pyfunction!(iterate_with_build_fixers))?;
    m.add_wrapped(wrap_pyfunction!(resolve_error))?;
    m.add_wrapped(wrap_pyfunction!(shebang_binary))?;
    m.add_class::<Session>()?;
    m.add_class::<PlainSession>()?;
    #[cfg(target_os = "linux")]
    m.add_class::<SchrootSession>()?;
    #[cfg(target_os = "linux")]
    m.add_class::<UnshareSession>()?;
    m.add_wrapped(wrap_pyfunction!(which))?;
    m.add_wrapped(wrap_pyfunction!(get_user))?;
    m.add("NoSessionOpen", py.get_type_bound::<NoSessionOpen>())?;
    m.add(
        "SessionAlreadyOpen",
        py.get_type_bound::<SessionAlreadyOpen>(),
    )?;
    m.add(
        "SessionSetupFailure",
        py.get_type_bound::<SessionSetupFailure>(),
    )?;
    m.add_wrapped(wrap_pyfunction!(run_with_tee))?;
    m.add("DistNoTarball", py.get_type_bound::<DistNoTarball>())?;
    Ok(())
}
