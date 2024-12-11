use std::collections::HashMap;
use std::io::{BufRead, Write};
use std::process::ExitStatus;

pub mod plain;
#[cfg(target_os = "linux")]
pub mod schroot;
#[cfg(target_os = "linux")]
pub mod unshare;

#[derive(Debug)]
pub enum Error {
    CalledProcessError(ExitStatus),
    IoError(std::io::Error),
    SetupFailure(String, String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Error::IoError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::CalledProcessError(code) => write!(f, "CalledProcessError({})", code),
            Error::IoError(e) => write!(f, "IoError({})", e),
            Error::SetupFailure(msg, _long_description) => write!(f, "SetupFailure({})", msg),
        }
    }
}

impl std::error::Error for Error {}

pub trait Session {
    /// Change the current working directory in the session.
    fn chdir(&mut self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    fn pwd(&self) -> &std::path::Path;

    /// Return the external path for a path inside the session.
    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf;

    /// Return the location of the session.
    fn location(&self) -> std::path::PathBuf;

    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error>;

    /// Ensure that the current users' home directory exists.
    fn create_home(&self) -> Result<(), Error>;

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), crate::session::Error>;

    /// Check if a file or directory exists.
    fn exists(&self, path: &std::path::Path) -> bool;

    /// Create a directory.
    fn mkdir(&self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    /// Recursively remove a directory.
    fn rmtree(&self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    /// Setup a project from an existing directory.
    ///
    /// # Arguments
    /// * `path` - The path to the directory to setup the session from.
    /// * `subdir` - The subdirectory to use as the session root.
    fn project_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<Project, Error>;

    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a>;

    fn popen(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        stdout: Option<std::process::Stdio>,
        stderr: Option<std::process::Stdio>,
        stdin: Option<std::process::Stdio>,
        env: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<std::process::Child, Error>;

    /// Check if the session is temporary.
    fn is_temporary(&self) -> bool;

    #[cfg(feature = "breezy")]
    /// Setup a project from a VCS tree.
    ///
    /// # Arguments
    /// * `tree` - The VCS tree to setup the session from.
    /// * `include_controldir` - Whether to include the control directory.
    /// * `subdir` - The subdirectory to use as the session root.
    ///
    /// # Returns
    /// A tuple containing the path to the tree in the session and
    /// the external path.
    fn project_from_vcs(
        &self,
        tree: &dyn crate::vcs::DupableTree,
        include_controldir: Option<bool>,
        subdir: Option<&str>,
    ) -> Result<Project, Error>;

    fn read_dir(&self, path: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, Error>;
}

pub enum Project {
    /// A project that does not need to be cleaned up.
    Noop(std::path::PathBuf),

    /// A temporary project that needs to be cleaned up.
    Temporary {
        external_path: std::path::PathBuf,
        internal_path: std::path::PathBuf,
        td: std::path::PathBuf,
    },
}

impl Drop for Project {
    fn drop(&mut self) {
        match self {
            Project::Noop(_) => {}
            Project::Temporary {
                external_path: _,
                internal_path: _,
                td,
            } => {
                log::info!("Removing temporary project {}", td.display());
                std::fs::remove_dir_all(td).unwrap();
            }
        }
    }
}

impl Project {
    pub fn internal_path(&self) -> &std::path::Path {
        match self {
            Project::Noop(path) => path,
            Project::Temporary { internal_path, .. } => internal_path,
        }
    }

    pub fn external_path(&self) -> &std::path::Path {
        match self {
            Project::Noop(path) => path,
            Project::Temporary { external_path, .. } => external_path,
        }
    }
}

impl From<tempfile::TempDir> for Project {
    fn from(tempdir: tempfile::TempDir) -> Self {
        Project::Temporary {
            external_path: tempdir.path().to_path_buf(),
            internal_path: tempdir.path().to_path_buf(),
            td: tempdir.into_path(),
        }
    }
}

pub struct CommandBuilder<'a> {
    session: &'a dyn Session,
    argv: Vec<&'a str>,
    cwd: Option<&'a std::path::Path>,
    user: Option<&'a str>,
    env: Option<std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
    stdout: Option<std::process::Stdio>,
    stderr: Option<std::process::Stdio>,
    quiet: bool,
}

impl<'a> CommandBuilder<'a> {
    pub fn new(session: &'a dyn Session, argv: Vec<&'a str>) -> Self {
        CommandBuilder {
            session,
            argv,
            cwd: None,
            user: None,
            env: None,
            stdin: None,
            stdout: None,
            stderr: None,
            quiet: false,
        }
    }

    pub fn quiet(mut self, quiet: bool) -> Self {
        self.quiet = quiet;
        self
    }

    /// Set the current working directory for the command.
    pub fn cwd(mut self, cwd: &'a std::path::Path) -> Self {
        self.cwd = Some(cwd);
        self
    }

    /// Set the user to run the command as.
    pub fn user(mut self, user: &'a str) -> Self {
        self.user = Some(user);
        self
    }

    /// Set the environment for the command.
    pub fn env(mut self, env: std::collections::HashMap<String, String>) -> Self {
        assert!(self.env.is_none());
        self.env = Some(env);
        self
    }

    /// Add an environment variable to the command.
    pub fn setenv(mut self, key: String, value: String) -> Self {
        self.env = match self.env {
            Some(mut env) => {
                env.insert(key, value);
                Some(env)
            }
            None => Some(std::collections::HashMap::from([(key, value)])),
        };
        self
    }

    pub fn stdin(mut self, stdin: std::process::Stdio) -> Self {
        self.stdin = Some(stdin);
        self
    }

    pub fn stdout(mut self, stdout: std::process::Stdio) -> Self {
        self.stdout = Some(stdout);
        self
    }

    pub fn stderr(mut self, stderr: std::process::Stdio) -> Self {
        self.stderr = Some(stderr);
        self
    }

    pub fn run_with_tee(self) -> Result<(ExitStatus, Vec<String>), Error> {
        assert!(self.stdout.is_none());
        assert!(self.stderr.is_none());
        run_with_tee(
            self.session,
            self.argv,
            self.cwd,
            self.user,
            self.env.as_ref(),
            self.stdin,
            self.quiet,
        )
    }

    pub fn run_detecting_problems(self) -> Result<Vec<String>, crate::analyze::AnalyzedError> {
        assert!(self.stdout.is_none());
        assert!(self.stderr.is_none());
        crate::analyze::run_detecting_problems(
            self.session,
            self.argv,
            None,
            self.quiet,
            self.cwd,
            self.user,
            self.env.as_ref(),
            self.stdin,
        )
    }

    pub fn run_fixing_problems<
        I: std::error::Error,
        E: From<I> + std::error::Error + From<std::io::Error>,
    >(
        self,
        fixers: &[&dyn crate::fix_build::BuildFixer<I>],
    ) -> Result<Vec<String>, crate::fix_build::IterateBuildError<E>> {
        assert!(self.stdin.is_none());
        assert!(self.stdout.is_none());
        assert!(self.stderr.is_none());
        crate::fix_build::run_fixing_problems(
            fixers,
            None,
            self.session,
            self.argv.as_slice(),
            self.quiet,
            self.cwd,
            self.user,
            self.env.as_ref(),
        )
    }

    pub fn child(self) -> Result<std::process::Child, Error> {
        self.session.popen(
            self.argv,
            self.cwd,
            self.user,
            self.stdout,
            self.stderr,
            self.stdin,
            self.env.as_ref(),
        )
    }

    pub fn run(self) -> Result<std::process::ExitStatus, Error> {
        let mut p = self.child()?;
        let status = p.wait()?;
        Ok(status)
    }

    pub fn output(self) -> Result<std::process::Output, Error> {
        let p = self.child()?;
        let output = p.wait_with_output()?;
        Ok(output)
    }

    pub fn check_call(self) -> Result<(), Error> {
        self.session
            .check_call(self.argv, self.cwd, self.user, self.env)
    }

    pub fn check_output(self) -> Result<Vec<u8>, Error> {
        self.session
            .check_output(self.argv, self.cwd, self.user, self.env)
    }
}

pub fn which(session: &dyn Session, name: &str) -> Option<String> {
    let ret = match session.check_output(
        vec!["which", name],
        Some(std::path::Path::new("/")),
        None,
        None,
    ) {
        Ok(ret) => ret,
        Err(Error::CalledProcessError(status)) if status.code() == Some(1) => return None,
        Err(e) => panic!("Unexpected error: {:?}", e),
    };
    if ret.is_empty() {
        None
    } else {
        Some(String::from_utf8(ret).unwrap().trim().to_string())
    }
}

pub fn get_user(session: &dyn Session) -> String {
    String::from_utf8(
        session
            .check_output(
                vec!["sh", "-c", "echo $USER"],
                Some(std::path::Path::new("/")),
                None,
                None,
            )
            .unwrap(),
    )
    .unwrap()
    .trim()
    .to_string()
}

/// A function to capture and forward stdout and stderr of a child process.
fn capture_output(
    mut child: std::process::Child,
    forward: bool,
) -> Result<(std::process::ExitStatus, Vec<String>), std::io::Error> {
    use std::io::{BufRead, BufReader};
    use std::sync::mpsc::{channel, Receiver, Sender};
    use std::thread;
    let mut output_log = Vec::<String>::new();

    // Channels to handle communication from threads
    let (tx, rx): (Sender<Option<String>>, Receiver<Option<String>>) = channel();

    // Function to handle the stdout of the child process
    let stdout_tx = tx.clone();
    let stdout = child.stdout.take().expect("Failed to capture stdout");
    let stdout_handle = thread::spawn(move || -> Result<(), std::io::Error> {
        let reader = BufReader::new(stdout);
        for line in reader.lines() {
            let line = line?;
            if forward {
                std::io::stdout().write_all(line.as_bytes())?;
                std::io::stdout().write_all(b"\n")?;
            }
            stdout_tx
                .send(Some(line))
                .expect("Failed to send stdout through channel");
        }

        stdout_tx
            .send(None)
            .expect("Failed to send None through channel");
        Ok(())
    });

    // Function to handle the stderr of the child process
    let stderr_tx = tx.clone();
    let stderr = child.stderr.take().expect("Failed to capture stderr");
    let stderr_handle = thread::spawn(move || -> Result<(), std::io::Error> {
        let reader = BufReader::new(stderr);
        for line in reader.lines() {
            let line = line?;
            if forward {
                std::io::stderr().write_all(line.as_bytes())?;
                std::io::stderr().write_all(b"\n")?;
            }
            stderr_tx
                .send(Some(line))
                .expect("Failed to send stderr through channel");
        }
        stderr_tx
            .send(None)
            .expect("Failed to send None through channel");
        Ok(())
    });

    // Wait for the child process to exit
    let status = child.wait().expect("Child process wasn't running");
    stderr_handle
        .join()
        .expect("Failed to join stderr thread")?;
    stdout_handle
        .join()
        .expect("Failed to join stdout thread")?;

    let mut terminated = 0;

    // Collect all output from both stdout and stderr
    while let Ok(line) = rx.recv() {
        if let Some(line) = line {
            output_log.push(line);
        } else {
            terminated += 1;
            if terminated == 2 {
                break;
            }
        }
    }

    Ok((status, output_log))
}

pub fn run_with_tee(
    session: &dyn Session,
    args: Vec<&str>,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
    quiet: bool,
) -> Result<(ExitStatus, Vec<String>), Error> {
    if let (Some(cwd), Some(user)) = (cwd, user) {
        log::debug!("Running command: {:?} in {:?} as user {}", args, cwd, user);
    } else if let Some(cwd) = cwd {
        log::debug!("Running command: {:?} in {:?}", args, cwd);
    } else if let Some(user) = user {
        log::debug!("Running command: {:?} as user {}", args, user);
    } else {
        log::debug!("Running command: {:?}", args);
    }
    let p = session.popen(
        args,
        cwd,
        user,
        Some(std::process::Stdio::piped()),
        Some(std::process::Stdio::piped()),
        Some(stdin.unwrap_or(std::process::Stdio::null())),
        env,
    )?;
    // While the process is running, read its output and write it to stdout
    // *and* to the contents variable.
    Ok(capture_output(p, !quiet)?)
}

pub fn create_home(session: &impl Session) -> Result<(), Error> {
    let cwd = std::path::Path::new("/");
    let home = String::from_utf8(session.check_output(
        vec!["sh", "-c", "echo $HOME"],
        Some(cwd),
        None,
        None,
    )?)
    .unwrap()
    .trim_end_matches('\n')
    .to_string();
    let user = String::from_utf8(session.check_output(
        vec!["sh", "-c", "echo $LOGNAME"],
        Some(cwd),
        None,
        None,
    )?)
    .unwrap()
    .trim_end_matches('\n')
    .to_string();
    log::info!("Creating directory {} in schroot session.", home);
    session.check_call(vec!["mkdir", "-p", &home], Some(cwd), Some("root"), None)?;
    session.check_call(vec!["chown", &user, &home], Some(cwd), Some("root"), None)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_get_user() {
        let session = super::plain::PlainSession::new();
        let user = super::get_user(&session);
        assert!(!user.is_empty());
    }

    #[test]
    fn test_which() {
        let session = super::plain::PlainSession::new();
        let which = super::which(&session, "ls");
        assert!(which.unwrap().ends_with("/ls"));
    }

    #[test]
    fn test_capture_and_forward_output() {
        let p = std::process::Command::new("echo")
            .arg("Hello, world!")
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .spawn()
            .unwrap();

        let (status, output) = super::capture_output(p, false).unwrap();
        assert!(status.success());
        assert_eq!(output, vec!["Hello, world!"]);
    }
}
