use std::collections::HashMap;

pub mod schroot;
pub mod plain;

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

    fn setup_from_directory(&self, path: &std::path::Path, subdir: Option<&str>) -> Result<(std::path::PathBuf, std::path::PathBuf), Error>;

}

pub fn which(session: &impl Session, name: &str) -> Option<String> {
    let ret = match session.check_output(vec!["which", name], Some(std::path::Path::new("/")), None, None) {
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

pub fn get_user(session: &impl Session) -> String {
    String::from_utf8(
    session.check_output(vec!["sh", "-c", "echo $USER"], Some(std::path::Path::new("/")), None, None).unwrap()).unwrap()
        .trim().to_string()
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
