pub struct PlainSession;
use crate::session::{Error, Session};

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
        std::fs::create_dir_all(path).map_err(|e| Error::IoError(e))
    }

    fn chdir(&mut self, path: &std::path::Path) -> Result<(), Error> {
        std::env::set_current_dir(path).map_err(|e| Error::IoError(e))
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
        let mut cmd = binding
            .args(&argv[1..]);

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
                    Err(Error::CalledProcessError(
                        output.status.code().unwrap(),
                    ))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn rmtree(&self, path: &std::path::Path) -> Result<(), Error> {
        std::fs::remove_dir_all(path).map_err(|e| Error::IoError(e))
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
        let mut cmd = binding
            .args(&argv[1..]);

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

    fn setup_from_directory(&self, path: &std::path::Path, _subdir: Option<&str>) -> Result<(std::path::PathBuf, std::path::PathBuf), Error> {
        Ok((path.into(), path.into()))
    }
}
