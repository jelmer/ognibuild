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
