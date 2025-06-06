use std::collections::HashMap;
use std::io::Write;
use std::process::ExitStatus;

/// Plain session implementation.
pub mod plain;
/// Schroot session implementation (Linux only).
#[cfg(target_os = "linux")]
pub mod schroot;
/// Unshare session implementation (Linux only).
#[cfg(target_os = "linux")]
pub mod unshare;

#[derive(Debug)]
/// Errors that can occur in a session.
pub enum Error {
    /// Error caused by a command that exited with a non-zero status code.
    CalledProcessError(ExitStatus),
    /// Error from an IO operation.
    IoError(std::io::Error),
    /// Error from setting up the session, with a message and detailed description.
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

/// Session interface for running commands in different environments.
///
/// This trait defines the interface for running commands in different environments,
/// such as the local system, a chroot, or a container.
pub trait Session {
    /// Change the current working directory in the session.
    fn chdir(&mut self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    /// Get the current working directory in the session.
    ///
    /// # Returns
    /// The current working directory
    fn pwd(&self) -> &std::path::Path;

    /// Return the external path for a path inside the session.
    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf;

    /// Return the location of the session.
    fn location(&self) -> std::path::PathBuf;

    /// Run a command and return its output.
    ///
    /// This method runs a command in the session and returns its output
    /// if the command exits successfully.
    ///
    /// # Arguments
    /// * `argv` - The command and its arguments
    /// * `cwd` - Optional current working directory
    /// * `user` - Optional user to run the command as
    /// * `env` - Optional environment variables
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - The command output if successful
    /// * `Err(Error)` - If the command fails
    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error>;

    /// Ensure that the current users' home directory exists.
    fn create_home(&self) -> Result<(), Error>;

    /// Run a command and check that it exits successfully.
    ///
    /// This method runs a command in the session and returns success
    /// if the command exits with a zero status code.
    ///
    /// # Arguments
    /// * `argv` - The command and its arguments
    /// * `cwd` - Optional current working directory
    /// * `user` - Optional user to run the command as
    /// * `env` - Optional environment variables
    ///
    /// # Returns
    /// * `Ok(())` - If the command exited successfully
    /// * `Err(Error)` - If the command fails
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

    /// Create a new command builder for the session.
    ///
    /// # Arguments
    /// * `argv` - The command and its arguments
    ///
    /// # Returns
    /// A new CommandBuilder instance
    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a>;

    /// Start a process in the session.
    ///
    /// # Arguments
    /// * `argv` - The command and its arguments
    /// * `cwd` - Optional current working directory
    /// * `user` - Optional user to run the command as
    /// * `stdout` - Optional stdout configuration
    /// * `stderr` - Optional stderr configuration
    /// * `stdin` - Optional stdin configuration
    /// * `env` - Optional environment variables
    ///
    /// # Returns
    /// * `Ok(Child)` - A handle to the running process
    /// * `Err(Error)` - If starting the process fails
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

    /// Read the contents of a directory.
    ///
    /// # Arguments
    /// * `path` - Path to the directory to read
    ///
    /// # Returns
    /// * `Ok(Vec<DirEntry>)` - The directory entries if successful
    /// * `Err(Error)` - If reading the directory fails
    fn read_dir(&self, path: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, Error>;
}

/// Represents a project in a session, either as a temporary copy or a direct reference.
pub enum Project {
    /// A project that does not need to be cleaned up.
    Noop(std::path::PathBuf),

    /// A temporary project that needs to be cleaned up.
    /// A temporary copy of a project, which exists only for the duration of the session.
    Temporary {
        /// The path to the project from the external environment.
        external_path: std::path::PathBuf,
        /// The path to the project inside the session.
        internal_path: std::path::PathBuf,
        /// The path to the temporary directory.
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
    /// Get the path to the project inside the session.
    ///
    /// # Returns
    /// The path to the project inside the session
    pub fn internal_path(&self) -> &std::path::Path {
        match self {
            Project::Noop(path) => path,
            Project::Temporary { internal_path, .. } => internal_path,
        }
    }

    /// Get the path to the project from the external environment.
    ///
    /// # Returns
    /// The path to the project from the external environment
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

/// Builder for creating and running commands in a session.
///
/// This struct provides a fluent interface for configuring and executing
/// commands within a session, handling options like working directory,
/// environment variables, input/output redirection, and more.
pub struct CommandBuilder<'a> {
    /// The session to run the command in
    session: &'a dyn Session,
    /// The command and its arguments
    argv: Vec<&'a str>,
    /// Optional current working directory
    cwd: Option<&'a std::path::Path>,
    /// Optional user to run the command as
    user: Option<&'a str>,
    /// Optional environment variables
    env: Option<std::collections::HashMap<String, String>>,
    /// Optional stdin configuration
    stdin: Option<std::process::Stdio>,
    /// Optional stdout configuration
    stdout: Option<std::process::Stdio>,
    /// Optional stderr configuration
    stderr: Option<std::process::Stdio>,
    /// Whether to suppress output
    quiet: bool,
}

impl<'a> CommandBuilder<'a> {
    /// Create a new CommandBuilder.
    ///
    /// # Arguments
    /// * `session` - The session to run the command in
    /// * `argv` - The command and its arguments
    ///
    /// # Returns
    /// A new CommandBuilder instance
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

    /// Set whether the command should run quietly.
    ///
    /// # Arguments
    /// * `quiet` - Whether to suppress output
    ///
    /// # Returns
    /// Self for method chaining
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

    /// Set the stdin for the command.
    ///
    /// # Arguments
    /// * `stdin` - The stdin configuration
    ///
    /// # Returns
    /// Self for method chaining
    pub fn stdin(mut self, stdin: std::process::Stdio) -> Self {
        self.stdin = Some(stdin);
        self
    }

    /// Set the stdout for the command.
    ///
    /// # Arguments
    /// * `stdout` - The stdout configuration
    ///
    /// # Returns
    /// Self for method chaining
    pub fn stdout(mut self, stdout: std::process::Stdio) -> Self {
        self.stdout = Some(stdout);
        self
    }

    /// Set the stderr for the command.
    ///
    /// # Arguments
    /// * `stderr` - The stderr configuration
    ///
    /// # Returns
    /// Self for method chaining
    pub fn stderr(mut self, stderr: std::process::Stdio) -> Self {
        self.stderr = Some(stderr);
        self
    }

    /// Run the command and capture its output, while also displaying it.
    ///
    /// This method executes the command and collects its output, while also
    /// displaying it in real time.
    ///
    /// # Returns
    /// * `Ok((ExitStatus, Vec<String>))` - The exit status and output lines if successful
    /// * `Err(Error)` - If the command fails
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

    /// Run the command and analyze the output for problems.
    ///
    /// This method executes the command and analyzes its output for common
    /// build problems, returning a more detailed error when issues are detected.
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - The output lines if successful
    /// * `Err(AnalyzedError)` - A detailed error if the command fails
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

    /// Run the command and attempt to fix any problems that occur.
    ///
    /// This method executes the command and applies fixes if it fails,
    /// potentially retrying multiple times with different fixers.
    ///
    /// # Arguments
    /// * `fixers` - List of fixers to try if the command fails
    ///
    /// # Returns
    /// * `Ok(Vec<String>)` - The command output if successful
    /// * `Err(IterateBuildError)` - If the command fails and can't be fixed
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

    /// Start the command and return a handle to the running process.
    ///
    /// # Returns
    /// * `Ok(Child)` - A handle to the running process
    /// * `Err(Error)` - If starting the process fails
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

    /// Run the command and return its exit status.
    ///
    /// # Returns
    /// * `Ok(ExitStatus)` - The exit status if successful
    /// * `Err(Error)` - If the command fails
    pub fn run(self) -> Result<std::process::ExitStatus, Error> {
        let mut p = self.child()?;
        let status = p.wait()?;
        Ok(status)
    }

    /// Run the command and return its output.
    ///
    /// # Returns
    /// * `Ok(Output)` - The command output if successful
    /// * `Err(Error)` - If the command fails
    pub fn output(self) -> Result<std::process::Output, Error> {
        let p = self.child()?;
        let output = p.wait_with_output()?;
        Ok(output)
    }

    /// Run the command and check that it exits successfully.
    ///
    /// # Returns
    /// * `Ok(())` - If the command exited successfully
    /// * `Err(Error)` - If the command fails
    pub fn check_call(self) -> Result<(), Error> {
        self.session
            .check_call(self.argv, self.cwd, self.user, self.env)
    }

    /// Run the command and return its output.
    ///
    /// # Returns
    /// * `Ok(Vec<u8>)` - The command output if successful
    /// * `Err(Error)` - If the command fails
    pub fn check_output(self) -> Result<Vec<u8>, Error> {
        self.session
            .check_output(self.argv, self.cwd, self.user, self.env)
    }
}

/// Find the path to an executable in the session's PATH.
///
/// # Arguments
/// * `session` - The session to search in
/// * `name` - The name of the executable to find
///
/// # Returns
/// The full path to the executable if found, or None if not found
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

/// Get the current user in the session.
///
/// # Arguments
/// * `session` - The session to get the user from
///
/// # Returns
/// The username of the current user
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

/// Run a command and capture its output, while also displaying it.
///
/// This function executes a command in the given session and collects
/// its output, while also displaying it in real time.
///
/// # Arguments
/// * `session` - The session to run the command in
/// * `args` - The command and its arguments
/// * `cwd` - Optional current working directory
/// * `user` - Optional user to run the command as
/// * `env` - Optional environment variables
/// * `stdin` - Optional stdin configuration
/// * `quiet` - Whether to suppress output
///
/// # Returns
/// * `Ok((ExitStatus, Vec<String>))` - The exit status and output lines if successful
/// * `Err(Error)` - If the command fails
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

/// Create the user's home directory in the session.
///
/// This function creates the user's home directory in the session,
/// which is needed for some commands that write to the home directory.
///
/// # Arguments
/// * `session` - The session to create the home directory in
///
/// # Returns
/// * `Ok(())` if the home directory was created successfully
/// * `Err(Error)` if creating the home directory fails
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
