use crate::session::Error;
use std::io::{BufRead, Read};

extern crate rand;
use rand::Rng;
use std::iter;

pub fn sanitize_session_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c.is_alphanumeric() || "_-.".contains(c))
        .collect()
}

pub fn generate_session_id(prefix: &str) -> String {
    let suffix: String = String::from_utf8(
        iter::repeat(())
            .map(|()| rand::thread_rng().sample(rand::distributions::Alphanumeric))
            .take(8)
            .collect(),
    )
    .unwrap();
    format!("{}-{}", sanitize_session_name(prefix), suffix)
}

pub struct SchrootSession {
    cwd: Option<String>,
    session_id: String,
}

impl SchrootSession {
    pub fn new(chroot: &str, session_prefix: Option<&str>) -> Result<Self, Error> {
        let mut stderr = tempfile::tempfile().unwrap();
        let mut extra_args = vec![];
        if let Some(session_prefix) = session_prefix {
            let sanitized_session_name = generate_session_id(session_prefix);
            extra_args.extend(["-n".to_string(), sanitized_session_name]);
        }
        let cmd = std::process::Command::new("schroot")
            .arg("-c")
            .arg(chroot)
            .arg("-b")
            .args(extra_args)
            .stderr(std::process::Stdio::from(stderr.try_clone().unwrap()))
            .output()
            .unwrap();

        let session_id = match cmd.status.code() {
            Some(0) => { String::from_utf8(cmd.stdout).unwrap() },
            Some(_) => {
                let mut errlines = String::new();
                stderr.read_to_string(&mut errlines).unwrap();
                if errlines.len() == 1 {
                    return Err(Error::SetupFailure(
                        errlines.lines().next().unwrap().to_string(),
                        errlines,
                    ));
                } else if errlines.len() == 0 {
                    return Err(Error::SetupFailure(
                        "No output from schroot".to_string(),
                        errlines,
                    ));
                } else {
                    return Err(Error::SetupFailure(
                        errlines.lines().last().unwrap().to_string(),
                        errlines,
                    ));
                }
            }
            None => panic!("schroot exited by signal"),
        };

        log::info!(
            "Opened schroot session {} (from {})", session_id, chroot
        );

        Ok(Self {
            cwd: None,
            session_id,
        })
    }

    fn run_argv(
        &self,
        argv: Vec<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>
    ) -> Vec<String> {
        let mut argv  = argv.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let mut base_argv = vec!["schroot".to_string(), "-r".to_string(), "-c".to_string(), format!("session:{}", self.session_id)];
        let cwd = cwd.or(self.cwd.as_deref());

        if let Some(cwd) = cwd {
            base_argv.extend(["-d".to_string(), cwd.to_string()]);
        }
        if let Some(user) = user {
            base_argv.extend(["-u".to_string(), user.to_string()]);
        }
        if let Some(env) = env {
            argv = vec![
                "sh".to_string(),
                "-c".to_string(),
                env.iter().map(|(key, value)| format!("{}={} ", key, shlex::try_quote(value).unwrap().to_string())).chain(argv.iter().map(|x| shlex::try_quote(x).unwrap().to_string())).collect::<Vec<String>>().join(" ")
            ];
        }
        [base_argv, vec!["--".to_string()], argv].concat()
    }
}

impl Drop for SchrootSession {
    fn drop(&mut self) {
        let stderr = tempfile::tempfile().unwrap();
        match std::process::Command::new("schroot")
            .arg("-c")
            .arg(format!("session:{}", self.session_id))
            .arg("-e")
            .stderr(std::process::Stdio::from(stderr.try_clone().unwrap()))
            .output() {
            Err(_) => {
                for line in std::io::BufReader::new(&stderr).lines() {
                    let line = line.unwrap();
                    if line.starts_with(&"E: ") {
                        log::error!("{}", &line[3..]);
                    }
                }
                log::error!("Failed to close schroot session {}, leaving stray.", self.session_id);
            }
            Ok(_) => {
                log::debug!("Closed schroot session {}", self.session_id);
            }
        }
    }
}

impl crate::session::Session for SchrootSession {
    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&str>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Vec<u8>, Error> {
        let argv = self.run_argv(argv, cwd, user, env.as_ref());

        let output = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .output();

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
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_sanitize_session_name() {
        assert_eq!(super::sanitize_session_name("foo"), "foo");
        assert_eq!(super::sanitize_session_name("foo-bar"), "foo-bar");
        assert_eq!(super::sanitize_session_name("foo_bar"), "foo_bar");
        assert_eq!(super::sanitize_session_name("foo.bar"), "foo.bar");
        assert_eq!(super::sanitize_session_name("foo!bar"), "foobar");
        assert_eq!(super::sanitize_session_name("foo@bar"), "foobar");
    }

    #[test]
    fn test_generate_session_id() {
        let id = super::generate_session_id("foo");
        assert_eq!(id.len(), 12);
        assert_eq!(&id[..4], "foo-");
    }
}
