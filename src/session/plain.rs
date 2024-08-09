pub struct PlainSession;
use crate::session::{Error, Session};

impl Default for PlainSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PlainSession {
    pub fn new() -> Self {
        PlainSession
    }

    fn prepend_user<'a>(&'a self, user: Option<&'a str>, mut args: Vec<&'a str>) -> Vec<&'a str> {
        if let Some(user) = user {
            if user != whoami::username() {
                args = vec!["sudo", "-u", user].into_iter().chain(args).collect();
            }
        }
        args
    }
}

impl Session for PlainSession {
    fn location(&self) -> std::path::PathBuf {
        std::path::PathBuf::from("/")
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        std::path::Path::new(path).exists()
    }

    fn mkdir(&self, path: &std::path::Path) -> Result<(), Error> {
        std::fs::create_dir_all(path).map_err(Error::IoError)
    }

    fn chdir(&mut self, path: &std::path::Path) -> Result<(), Error> {
        std::env::set_current_dir(path).map_err(Error::IoError)
    }

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        std::path::PathBuf::from(path).canonicalize().unwrap()
    }

    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error> {
        let argv = self.prepend_user(user, argv);
        let mut binding = std::process::Command::new(argv[0]);
        let mut cmd = binding.args(&argv[1..]);

        if let Some(cwd) = cwd {
            cmd = cmd.current_dir(cwd);
        }

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        let output = cmd.output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(output.stdout)
                } else {
                    Err(Error::CalledProcessError(output.status.code().unwrap()))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn rmtree(&self, path: &std::path::Path) -> Result<(), Error> {
        std::fs::remove_dir_all(path).map_err(Error::IoError)
    }

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), Error> {
        let argv = self.prepend_user(user, argv);
        let mut binding = std::process::Command::new(argv[0]);
        let mut cmd = binding.args(&argv[1..]);

        if let Some(cwd) = cwd {
            cmd = cmd.current_dir(cwd);
        }

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        let status = cmd.status();

        match status {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::CalledProcessError(status.code().unwrap()))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn create_home(&self) -> Result<(), Error> {
        Ok(())
    }

    fn setup_from_directory(
        &self,
        path: &std::path::Path,
        _subdir: Option<&str>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), Error> {
        Ok((path.into(), path.into()))
    }

    fn popen(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        stdout: Option<std::process::Stdio>,
        stderr: Option<std::process::Stdio>,
        stdin: Option<std::process::Stdio>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> std::process::Child {
        let argv = self.prepend_user(user, argv);

        let mut binding = std::process::Command::new(argv[0]);

        let mut cmd = binding
            .args(&argv[1..])
            .stdin(stdin.unwrap_or(std::process::Stdio::inherit()))
            .stdout(stdout.unwrap_or(std::process::Stdio::inherit()))
            .stderr(stderr.unwrap_or(std::process::Stdio::inherit()));

        if let Some(cwd) = cwd {
            cmd = cmd.current_dir(cwd);
        }

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        cmd.spawn().unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_prepend_user() {
        let session = PlainSession::new();
        let args = vec!["ls"];
        let args = session.prepend_user(Some("root"), args);
        assert_eq!(args, vec!["sudo", "-u", "root", "ls"]);
    }

    #[test]
    fn test_prepend_user_no_user() {
        let session = PlainSession::new();
        let args = vec!["ls"];
        let args = session.prepend_user(None, args);
        assert_eq!(args, vec!["ls"]);
    }

    #[test]
    fn test_prepend_user_current_user() {
        let session = PlainSession::new();
        let args = vec!["ls"];
        let username = whoami::username();
        let args = session.prepend_user(Some(username.as_str()), args);
        assert_eq!(args, vec!["ls"]);
    }

    #[test]
    fn test_location() {
        let session = PlainSession::new();
        assert_eq!(session.location(), std::path::PathBuf::from("/"));
    }

    #[test]
    fn test_exists() {
        let session = PlainSession::new();
        assert!(session.exists(std::path::Path::new("/")));

        let td = tempfile::tempdir().unwrap();
        assert!(session.exists(td.path()));

        let path = td.path().join("test");
        assert!(!session.exists(&path));
    }

    #[test]
    fn test_mkdir() {
        let session = PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test");
        session.mkdir(&path).unwrap();
        assert!(session.exists(&path));
        session.rmtree(&path).unwrap();
        assert!(!session.exists(&path));
    }

    #[test]
    fn test_chdir() {
        let mut session = PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test");
        session.mkdir(&path).unwrap();
        session.chdir(&path).unwrap();
        assert_eq!(
            session
                .check_output(vec!["pwd"], None, None, None)
                .unwrap()
                .as_slice()
                .strip_suffix(b"\n")
                .unwrap(),
            path.canonicalize().unwrap().to_str().unwrap().as_bytes()
        );
    }

    #[test]
    fn test_external_path() {
        let session = PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test");
        session.mkdir(&path).unwrap();
        assert_eq!(session.external_path(&path), path.canonicalize().unwrap());
    }

    #[test]
    fn test_check_output() {
        let session = PlainSession::new();
        let output = session
            .check_output(vec!["echo", "hello"], None, None, None)
            .unwrap();
        assert_eq!(output, b"hello\n");
    }

    #[test]
    fn test_check_call() {
        let session = PlainSession::new();
        session.check_call(vec!["true"], None, None, None).unwrap();
    }

    #[test]
    fn test_create_home() {
        let session = PlainSession::new();
        session.create_home().unwrap();
    }

    #[test]
    fn test_setup_from_directory() {
        let session = PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test");
        session.mkdir(&path).unwrap();
        let (src, dest) = session.setup_from_directory(&path, None).unwrap();
        assert_eq!(src, path);
        assert_eq!(dest, path);
    }

    #[test]
    fn test_popen() {
        let session = PlainSession::new();
        let child = session.popen(
            vec!["echo", "hello"],
            None,
            None,
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
            None,
        );
        let output = child.wait_with_output().unwrap();
        assert_eq!(output.stdout, b"hello\n");
    }
}
