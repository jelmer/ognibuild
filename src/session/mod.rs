use std::collections::HashMap;
use std::io::{BufRead, Write};

pub mod plain;
#[cfg(target_os = "linux")]
pub mod schroot;
#[cfg(target_os = "linux")]
pub mod unshare;

#[derive(Debug)]
pub enum Error {
    CalledProcessError(i32),
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
    fn chdir(&mut self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf;

    fn location(&self) -> std::path::PathBuf;

    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error>;

    fn create_home(&self) -> Result<(), Error>;

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), crate::session::Error>;

    fn exists(&self, path: &std::path::Path) -> bool;

    fn mkdir(&self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    fn rmtree(&self, path: &std::path::Path) -> Result<(), crate::session::Error>;

    fn setup_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), Error>;

    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a>;

    fn popen(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        stdout: Option<std::process::Stdio>,
        stderr: Option<std::process::Stdio>,
        stdin: Option<std::process::Stdio>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> std::process::Child;

    fn is_temporary(&self) -> bool;

    fn setup_from_vcs(
        &self,
        tree: &dyn crate::vcs::DupableTree,
        include_controldir: Option<bool>,
        subdir: Option<&std::path::Path>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), Error>;
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
        }
    }

    pub fn cwd(mut self, cwd: &'a std::path::Path) -> Self {
        self.cwd = Some(cwd);
        self
    }

    pub fn user(mut self, user: &'a str) -> Self {
        self.user = Some(user);
        self
    }

    pub fn env(mut self, env: std::collections::HashMap<String, String>) -> Self {
        assert!(self.env.is_none());
        self.env = Some(env);
        self
    }

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

    pub fn run_with_tee(self) -> Result<(i32, Vec<String>), Error> {
        run_with_tee(
            self.session,
            self.argv,
            self.cwd,
            self.user,
            self.env,
            self.stdin,
            self.stdout,
            self.stderr,
        )
    }

    pub fn child(self) -> std::process::Child {
        self.session.popen(
            self.argv,
            self.cwd,
            self.user,
            self.stdout,
            self.stderr,
            self.stdin,
            self.env,
        )
    }

    pub fn run(self) -> Result<std::process::ExitStatus, Error> {
        let mut p = self.child();
        let status = p.wait()?;
        Ok(status)
    }

    pub fn output(self) -> Result<std::process::Output, Error> {
        let p = self.child();
        let output = p.wait_with_output()?;
        Ok(output)
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
        Err(Error::CalledProcessError(1)) => return None,
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

pub fn run_with_tee(
    session: &dyn Session,
    args: Vec<&str>,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
    stdout: Option<std::process::Stdio>,
    stderr: Option<std::process::Stdio>,
) -> Result<(i32, Vec<String>), Error> {
    let mut p = session.popen(
        args,
        cwd,
        user,
        stdout,
        stderr,
        Some(stdin.unwrap_or(std::process::Stdio::null())),
        env,
    );
    // While the process is running, read its output and write it to stdout
    // *and* to the contents variable.
    let mut contents = Vec::new();
    let stdout = p.stdout.as_mut().unwrap();
    let mut stdout_reader = std::io::BufReader::new(stdout);
    loop {
        let mut line = String::new();
        match stdout_reader.read_line(&mut line) {
            Ok(0) => break,
            Ok(_) => {
                std::io::stdout().write_all(line.as_bytes()).unwrap();
                contents.push(line);
            }
            Err(e) => {
                return Err(Error::IoError(e));
            }
        }
    }
    let status = p.wait().unwrap();
    Ok((status.code().unwrap(), contents))
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
}
