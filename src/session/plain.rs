pub struct PlainSession;

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

impl crate::session::Session for PlainSession {
    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Vec<u8>, crate::session::Error> {
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
                    Err(crate::session::Error::CalledProcessError(
                        output.status.code().unwrap(),
                    ))
                }
            }
            Err(e) => Err(crate::session::Error::IoError(e)),
        }
    }

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), crate::session::Error> {
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
                    Err(crate::session::Error::CalledProcessError(status.code().unwrap()))
                }
            }
            Err(e) => Err(crate::session::Error::IoError(e)),
        }
    }

    fn create_home(&self) -> Result<(), crate::session::Error> {
        Ok(())
    }
}
