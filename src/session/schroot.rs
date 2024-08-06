use crate::session::{Error, Session};
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
    cwd: Option<std::path::PathBuf>,
    session_id: String,
    location: std::path::PathBuf,
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
            Some(0) => String::from_utf8(cmd.stdout).unwrap(),
            Some(_) => {
                let mut errlines = String::new();
                stderr.read_to_string(&mut errlines).unwrap();
                if errlines.len() == 1 {
                    return Err(Error::SetupFailure(
                        errlines.lines().next().unwrap().to_string(),
                        errlines,
                    ));
                } else if errlines.is_empty() {
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

        log::info!("Opened schroot session {} (from {})", session_id, chroot);

        let output = std::process::Command::new("schroot")
            .arg("-c")
            .arg(format!("session:{}", session_id))
            .arg("--location")
            .output()
            .unwrap();
        let location = std::path::PathBuf::from(
            String::from_utf8(output.stdout)
                .unwrap()
                .trim_end_matches('\n'),
        );

        Ok(Self {
            cwd: None,
            session_id,
            location,
        })
    }

    fn run_argv(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<&std::collections::HashMap<String, String>>,
    ) -> Vec<String> {
        let mut argv = argv.iter().map(|x| x.to_string()).collect::<Vec<String>>();
        let mut base_argv = vec![
            "schroot".to_string(),
            "-r".to_string(),
            "-c".to_string(),
            format!("session:{}", self.session_id),
        ];
        let cwd = cwd.or(self.cwd.as_deref());

        if let Some(cwd) = cwd {
            base_argv.extend([
                "-d".to_string(),
                cwd.to_path_buf().to_string_lossy().to_string(),
            ]);
        }
        if let Some(user) = user {
            base_argv.extend(["-u".to_string(), user.to_string()]);
        }
        if let Some(env) = env {
            argv = vec![
                "sh".to_string(),
                "-c".to_string(),
                env.iter()
                    .map(|(key, value)| format!("{}={} ", key, shlex::try_quote(value).unwrap()))
                    .chain(
                        argv.iter()
                            .map(|x| shlex::try_quote(x).unwrap().to_string()),
                    )
                    .collect::<Vec<String>>()
                    .join(" "),
            ];
        }
        [base_argv, vec!["--".to_string()], argv].concat()
    }

    fn build_tempdir(&self) -> std::path::PathBuf {
        let build_dir = "/build";

        String::from_utf8(
            self.check_output(
                vec!["mktemp", "-d", "-p", build_dir],
                Some(std::path::Path::new("/")),
                None,
                None,
            )
            .unwrap(),
        )
        .unwrap()
        .trim_end_matches('\n')
        .to_string()
        .into()
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
            .output()
        {
            Err(_) => {
                for line in std::io::BufReader::new(&stderr).lines() {
                    let line = line.unwrap();
                    if line.starts_with("E: ") {
                        log::error!("{}", &line[3..]);
                    }
                }
                log::error!(
                    "Failed to close schroot session {}, leaving stray.",
                    self.session_id
                );
            }
            Ok(_) => {
                log::debug!("Closed schroot session {}", self.session_id);
            }
        }
    }
}

impl Session for SchrootSession {
    fn rmtree(&self, path: &std::path::Path) -> Result<(), Error> {
        let fullpath = self.external_path(path);
        std::fs::remove_dir_all(fullpath).map_err(Error::IoError)
    }

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        let path = path.to_string_lossy();
        if path.starts_with('/') {
            return self.location().join(path.trim_end_matches('/'));
        }
        if let Some(cwd) = &self.cwd {
            return self
                .location()
                .join(cwd.to_string_lossy().to_string().trim_start_matches('/'))
                .join(path.as_ref());
        } else {
            panic!("no cwd set");
        }
    }

    fn location(&self) -> std::path::PathBuf {
        self.location.clone()
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        let fullpath = self.external_path(path);
        fullpath.exists()
    }

    fn chdir(&mut self, path: &std::path::Path) -> Result<(), Error> {
        self.cwd = Some(path.to_path_buf());
        Ok(())
    }

    fn mkdir(&self, path: &std::path::Path) -> Result<(), Error> {
        let fullpath = self.external_path(path);
        std::fs::create_dir_all(fullpath).map_err(Error::IoError)
    }

    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
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
                    Err(Error::CalledProcessError(output.status.code().unwrap()))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), Error> {
        let argv = self.run_argv(argv, cwd, user, env.as_ref());

        let status = std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .status();

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
        let cwd = std::path::Path::new("/");
        let home = String::from_utf8(self.check_output(
            vec!["sh", "-c", "echo $HOME"],
            Some(cwd),
            None,
            None,
        )?)
        .unwrap()
        .trim_end_matches('\n')
        .to_string();
        let user = String::from_utf8(self.check_output(
            vec!["sh", "-c", "echo $LOGNAME"],
            Some(cwd),
            None,
            None,
        )?)
        .unwrap()
        .trim_end_matches('\n')
        .to_string();
        log::info!("Creating directory {} in schroot session.", home);
        self.check_call(vec!["mkdir", "-p", &home], Some(cwd), Some("root"), None)?;
        self.check_call(vec!["chown", &user, &home], Some(cwd), Some("root"), None)?;
        Ok(())
    }

    fn setup_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), Error> {
        let subdir = subdir.unwrap_or("package");
        let reldir = self.build_tempdir();
        let export_directory = self.external_path(&reldir).join(subdir);
        // Copy tree from path to export_directory

        let mut options = fs_extra::dir::CopyOptions::new();
        options.copy_inside = true; // Copy contents inside the source directory
        options.content_only = false; // Copy the entire directory
        options.skip_exist = false; // Skip if file already exists in the destination
        options.overwrite = true; // Overwrite files if they already exist
        options.buffer_size = 64000; // Buffer size in bytes
        options.depth = 0; // Recursion depth (0 for unlimited depth)

        // Perform the copy operation
        fs_extra::dir::copy(path, &export_directory, &options).unwrap();

        Ok((export_directory, reldir.join(subdir)))
    }

    fn Popen(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        stdout: Option<std::process::Stdio>,
        stderr: Option<std::process::Stdio>,
        stdin: Option<std::process::Stdio>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> std::process::Child {
        let argv = self.run_argv(argv, cwd, user, env.as_ref());

        std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(stdin.unwrap_or(std::process::Stdio::inherit()))
            .stdout(stdout.unwrap_or(std::process::Stdio::inherit()))
            .stderr(stderr.unwrap_or(std::process::Stdio::inherit()))
            .spawn()
            .unwrap()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
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
