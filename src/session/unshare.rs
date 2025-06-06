use crate::session::{CommandBuilder, Error, Project, Session};
use std::path::{Path, PathBuf};

/// An unshare based session
pub struct UnshareSession {
    root: PathBuf,
    _tempdir: Option<tempfile::TempDir>,
    cwd: PathBuf,
}

fn compression_flag(path: &Path) -> Result<Option<&str>, crate::session::Error> {
    match path.extension().unwrap().to_str().unwrap() {
        "tar" => Ok(None),
        "gz" => Ok(Some("-z")),
        "bz2" => Ok(Some("-j")),
        "xz" => Ok(Some("-J")),
        "zst" => Ok(Some("--zstd")),
        e => Err(crate::session::Error::SetupFailure(
            "unknown extension".to_string(),
            format!("unknown extension: {}", e),
        )),
    }
}

impl UnshareSession {
    /// Create a session from a tarball
    pub fn from_tarball(path: &Path) -> Result<Self, crate::session::Error> {
        let td = tempfile::tempdir().map_err(|e| {
            crate::session::Error::SetupFailure("tempdir failed".to_string(), e.to_string())
        })?;

        // Run tar within unshare to extract the tarball. This is necessary because
        // the tarball may contain files that are owned by a different user.
        //
        // However, the tar executable is not available within the unshare environment.
        // Therefore, we need to extract the tarball to a temporary directory and then
        // move it to the final location.
        let root = td.path();

        let f = std::fs::File::open(path).map_err(|e| {
            crate::session::Error::SetupFailure("open failed".to_string(), e.to_string())
        })?;

        let output = std::process::Command::new("unshare")
            .arg("--map-users=auto")
            .arg("--map-groups=auto")
            .arg("--fork")
            .arg("--pid")
            .arg("--mount-proc")
            .arg("--net")
            .arg("--uts")
            .arg("--ipc")
            .arg("--wd")
            .arg(root)
            .arg("--")
            .arg("tar")
            .arg("x")
            .arg(compression_flag(path)?.unwrap_or("--"))
            .stdin(std::process::Stdio::from(f))
            .stderr(std::process::Stdio::piped())
            .output()?;

        if !output.status.success() {
            let stderr = String::from_utf8(output.stderr).unwrap();
            return Err(crate::session::Error::SetupFailure(
                "tar failed".to_string(),
                stderr,
            ));
        }

        let s = Self {
            root: root.to_path_buf(),
            _tempdir: Some(td),
            cwd: std::path::PathBuf::from("/"),
        };

        s.ensure_current_user()?;

        Ok(s)
    }

    /// Save the session to a tarball
    pub fn save_to_tarball(&self, path: &Path) -> Result<(), crate::session::Error> {
        // Create the tarball from within the session, dumping it to stdout
        let mut child = self.popen(
            vec![
                "tar",
                "c",
                "--absolute-names",
                "--exclude",
                "/dev/*",
                "--exclude",
                "/proc/*",
                "--exclude",
                "/sys/*",
                compression_flag(path)?.unwrap_or("--"),
                "/",
            ],
            Some(std::path::Path::new("/")),
            Some("root"),
            Some(std::process::Stdio::piped()),
            None,
            None,
            None,
        )?;

        let f = std::fs::File::create(path).map_err(|e| {
            crate::session::Error::SetupFailure("create failed".to_string(), e.to_string())
        })?;

        let mut writer = std::io::BufWriter::new(f);

        std::io::copy(child.stdout.as_mut().unwrap(), &mut writer).map_err(|e| {
            crate::session::Error::SetupFailure("copy failed".to_string(), e.to_string())
        })?;

        if child.wait()?.success() {
            Ok(())
        } else {
            Err(crate::session::Error::SetupFailure(
                "tar failed".to_string(),
                "tar failed".to_string(),
            ))
        }
    }

    /// Bootstrap the session environment
    pub fn bootstrap() -> Result<Self, crate::session::Error> {
        let td = tempfile::tempdir().map_err(|e| {
            crate::session::Error::SetupFailure("tempdir failed".to_string(), e.to_string())
        })?;

        let root = td.path();
        std::process::Command::new("mmdebstrap")
            .current_dir(root)
            .arg("--mode=unshare")
            .arg("--variant=minbase")
            .arg("--quiet")
            .arg("sid")
            .arg(root)
            .arg("http://deb.debian.org/debian/")
            .status()
            .map_err(|e| {
                crate::session::Error::SetupFailure("mmdebstrap failed".to_string(), e.to_string())
            })?;

        let s = Self {
            root: root.to_path_buf(),
            _tempdir: Some(td),
            cwd: std::path::PathBuf::from("/"),
        };

        s.ensure_current_user()?;

        Ok(s)
    }

    /// Verify that the current user has an account in the session
    pub fn ensure_current_user(&self) -> Result<(), crate::session::Error> {
        // Ensure that the current user has an entry in /etc/passwd
        let user = whoami::username();
        let uid = nix::unistd::getuid().to_string();
        let gid = nix::unistd::getgid().to_string();

        match self.check_call(
            vec![
                "/usr/sbin/groupadd",
                "--force",
                "--non-unique",
                "--gid",
                &gid,
                user.as_str(),
            ],
            Some(std::path::Path::new("/")),
            Some("root"),
            None,
        ) {
            Ok(_) => {}
            Err(e) => panic!("Error: {:?}", e),
        }

        let child = self.popen(
            vec![
                "/usr/sbin/useradd",
                "--uid",
                &uid,
                "--gid",
                &gid,
                user.as_str(),
            ],
            Some(std::path::Path::new("/")),
            Some("root"),
            None,
            Some(std::process::Stdio::piped()),
            None,
            None,
        )?;

        match child.wait_with_output() {
            Ok(output) => {
                match output.status.code() {
                    // User created
                    Some(0) => Ok(()),
                    // Ignore if user already exists
                    Some(9) => Ok(()),
                    Some(4) => Ok(()),
                    _ => panic!(
                        "Error: {:?}: {}",
                        output.status,
                        String::from_utf8(output.stdout).unwrap()
                    ),
                }
            }
            Err(e) => panic!("Error: {:?}", e),
        }
    }

    /// Run a command in the session
    pub fn run_argv<'a>(
        &'a self,
        argv: Vec<&'a str>,
        cwd: Option<&'a std::path::Path>,
        user: Option<&'a str>,
    ) -> std::vec::Vec<&'a str> {
        let mut ret = vec![
            "unshare",
            "--map-users=auto",
            "--map-groups=auto",
            "--fork",
            "--pid",
            "--mount-proc",
            "--net",
            "--uts",
            "--ipc",
            "--root",
            self.root.to_str().unwrap(),
            "--wd",
            cwd.unwrap_or(&self.cwd).to_str().unwrap(),
        ];
        if let Some(user) = user {
            if user == "root" {
                ret.push("--map-root-user")
            } else {
                ret.push("--map-user");
                ret.push(user);
            }
        } else {
            ret.push("--map-current-user")
        }
        ret.push("--");
        ret.extend(argv);
        ret
    }

    fn build_tempdir(&self, user: Option<&str>) -> std::path::PathBuf {
        let build_dir = "/build";

        // Ensure that the build directory exists
        self.check_call(vec!["mkdir", "-p", build_dir], None, user, None)
            .unwrap();

        String::from_utf8(
            self.check_output(
                vec!["mktemp", "-d", format!("--tmpdir={}", build_dir).as_str()],
                Some(std::path::Path::new("/")),
                user,
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

impl Session for UnshareSession {
    fn chdir(&mut self, path: &std::path::Path) -> Result<(), crate::session::Error> {
        self.cwd = self.cwd.join(path);
        Ok(())
    }

    fn pwd(&self) -> &std::path::Path {
        &self.cwd
    }

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        if let Ok(rest) = path.strip_prefix("/") {
            return self.location().join(rest);
        }
        self.location()
            .join(
                self.cwd
                    .to_string_lossy()
                    .to_string()
                    .trim_start_matches('/'),
            )
            .join(path)
    }

    fn location(&self) -> std::path::PathBuf {
        self.root.clone()
    }

    fn check_output(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<Vec<u8>, super::Error> {
        let argv = self.run_argv(argv, cwd, user);

        let output = std::process::Command::new(argv[0])
            .args(&argv[1..])
            .stderr(std::process::Stdio::inherit())
            .envs(env.unwrap_or_default())
            .output();

        match output {
            Ok(output) => {
                if output.status.success() {
                    Ok(output.stdout)
                } else {
                    Err(Error::CalledProcessError(output.status))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn create_home(&self) -> Result<(), super::Error> {
        crate::session::create_home(self)
    }

    fn check_call(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        env: Option<std::collections::HashMap<String, String>>,
    ) -> Result<(), crate::session::Error> {
        let argv = self.run_argv(argv, cwd, user);

        let status = std::process::Command::new(argv[0])
            .args(&argv[1..])
            .envs(env.unwrap_or_default())
            .status();

        match status {
            Ok(status) => {
                if status.success() {
                    Ok(())
                } else {
                    Err(Error::CalledProcessError(status))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        let args = vec!["test", "-e", path.to_str().unwrap()];
        self.check_call(args, None, None, None).is_ok()
    }

    fn mkdir(&self, path: &std::path::Path) -> Result<(), crate::session::Error> {
        let args = vec!["mkdir", path.to_str().unwrap()];
        self.check_call(args, None, None, None)
    }

    fn rmtree(&self, path: &std::path::Path) -> Result<(), crate::session::Error> {
        let args = vec!["rm", "-rf", path.to_str().unwrap()];
        self.check_call(args, None, None, None)
    }

    fn project_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<Project, super::Error> {
        let subdir = subdir.unwrap_or("package");
        let reldir = self.build_tempdir(Some("root"));

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

        Ok(Project::Temporary {
            external_path: export_directory,
            internal_path: reldir.join(subdir),
            td: self.external_path(&reldir),
        })
    }

    fn popen(
        &self,
        argv: Vec<&str>,
        cwd: Option<&std::path::Path>,
        user: Option<&str>,
        stdout: Option<std::process::Stdio>,
        stderr: Option<std::process::Stdio>,
        stdin: Option<std::process::Stdio>,
        env: Option<&std::collections::HashMap<String, String>>,
    ) -> Result<std::process::Child, Error> {
        let argv = self.run_argv(argv, cwd, user);

        let mut binding = std::process::Command::new(argv[0]);
        let mut cmd = binding.args(&argv[1..]);

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        if let Some(stdin) = stdin {
            cmd = cmd.stdin(stdin);
        }

        if let Some(stdout) = stdout {
            cmd = cmd.stdout(stdout);
        }

        if let Some(stderr) = stderr {
            cmd = cmd.stderr(stderr);
        }

        Ok(cmd.spawn()?)
    }

    fn is_temporary(&self) -> bool {
        true
    }

    #[cfg(feature = "breezy")]
    fn project_from_vcs(
        &self,
        tree: &dyn crate::vcs::DupableTree,
        include_controldir: Option<bool>,
        subdir: Option<&str>,
    ) -> Result<Project, Error> {
        let reldir = self.build_tempdir(None);

        let subdir = subdir.unwrap_or("package");

        let export_directory = self.external_path(&reldir).join(subdir);
        if !include_controldir.unwrap_or(false) {
            tree.export_to(&export_directory, None).unwrap();
        } else {
            crate::vcs::dupe_vcs_tree(tree, &export_directory).unwrap();
        }

        Ok(Project::Temporary {
            external_path: export_directory,
            internal_path: reldir.join(subdir),
            td: self.external_path(&reldir),
        })
    }

    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a> {
        CommandBuilder::new(self, argv)
    }

    fn read_dir(&self, path: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, Error> {
        std::fs::read_dir(self.external_path(path))
            .map_err(Error::IoError)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::IoError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    lazy_static::lazy_static! {
        static ref TEST_SESSION: std::sync::Mutex<UnshareSession> = std::sync::Mutex::new(UnshareSession::bootstrap().unwrap());
    }

    fn test_session() -> Option<std::sync::MutexGuard<'static, UnshareSession>> {
        // Don't run tests if we're in github actions
        // TODO: check for ability to run unshare instead
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            return None;
        }
        Some(TEST_SESSION.lock().unwrap())
    }

    #[test]
    fn test_is_temporary() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        assert!(session.is_temporary());
    }

    #[test]
    fn test_chdir() {
        let mut session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        session.chdir(std::path::Path::new("/")).unwrap();
    }

    #[test]
    fn test_check_output() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        let output = String::from_utf8(
            session
                .check_output(vec!["ls"], Some(std::path::Path::new("/")), None, None)
                .unwrap(),
        )
        .unwrap();
        let dirs = output.split_whitespace().collect::<Vec<&str>>();
        assert!(dirs.contains(&"bin"));
        assert!(dirs.contains(&"dev"));
        assert!(dirs.contains(&"etc"));
        assert!(dirs.contains(&"home"));
        assert!(dirs.contains(&"lib"));
        assert!(dirs.contains(&"usr"));
        assert!(dirs.contains(&"proc"));

        assert_eq!(
            "root",
            String::from_utf8(
                session
                    .check_output(vec!["whoami"], None, Some("root"), None)
                    .unwrap()
            )
            .unwrap()
            .trim_end()
        );
        assert_eq!(
            // Get current process uid
            String::from_utf8(
                session
                    .check_output(vec!["id", "-u"], None, None, None)
                    .unwrap()
            )
            .unwrap()
            .trim_end(),
            String::from_utf8(
                session
                    .check_output(vec!["id", "-u"], None, None, None)
                    .unwrap()
            )
            .unwrap()
            .trim_end()
        );

        assert_eq!(
            "nobody",
            String::from_utf8(
                session
                    .check_output(vec!["whoami"], None, Some("nobody"), None)
                    .unwrap()
            )
            .unwrap()
            .trim_end()
        );
    }

    #[test]
    fn test_check_call() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        session
            .check_call(vec!["true"], Some(std::path::Path::new("/")), None, None)
            .unwrap();
    }

    #[test]
    fn test_create_home() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        session.create_home().unwrap();
    }

    fn save_and_reuse(name: &str) {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        let tempdir = tempfile::tempdir().unwrap();
        let path = tempdir.path().join(name);
        session.save_to_tarball(&path).unwrap();
        std::mem::drop(session);
        let session = UnshareSession::from_tarball(&path).unwrap();
        assert!(session.exists(std::path::Path::new("/bin")));
        // Verify that the session works
        let output = String::from_utf8(
            session
                .check_output(vec!["ls"], Some(std::path::Path::new("/")), None, None)
                .unwrap(),
        )
        .unwrap();
        let dirs = output.split_whitespace().collect::<Vec<&str>>();
        assert!(dirs.contains(&"bin"));
        assert!(dirs.contains(&"dev"));
        assert!(dirs.contains(&"etc"));
        assert!(dirs.contains(&"home"));
        assert!(dirs.contains(&"lib"));
    }

    #[test]
    fn test_save_and_reuse() {
        save_and_reuse("test.tar");
    }

    #[test]
    fn test_save_and_reuse_gz() {
        save_and_reuse("test.tar.gz");
    }

    #[test]
    fn test_mkdir_rmdir() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        let path = std::path::Path::new("/tmp/test");
        session.mkdir(path).unwrap();
        assert!(session.exists(path));
        session.rmtree(path).unwrap();
        assert!(!session.exists(path));
    }

    #[test]
    fn test_project_from_directory() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        let tempdir = tempfile::tempdir().unwrap();
        std::fs::write(tempdir.path().join("test"), "test").unwrap();
        let project = session
            .project_from_directory(tempdir.path(), None)
            .unwrap();
        assert!(project.external_path().exists());
        assert!(session.exists(project.internal_path()));
        session.rmtree(project.internal_path()).unwrap();
        assert!(!session.exists(project.internal_path()));
        assert!(!project.external_path().exists());
    }

    #[test]
    fn test_popen() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        let child = session
            .popen(
                vec!["ls"],
                Some(std::path::Path::new("/")),
                None,
                Some(std::process::Stdio::piped()),
                Some(std::process::Stdio::piped()),
                Some(std::process::Stdio::piped()),
                None,
            )
            .unwrap();
        let output = String::from_utf8(child.wait_with_output().unwrap().stdout).unwrap();
        let dirs = output.split_whitespace().collect::<Vec<&str>>();
        assert!(dirs.contains(&"etc"));
        assert!(dirs.contains(&"home"));
        assert!(dirs.contains(&"lib"));
        assert!(dirs.contains(&"usr"));
        assert!(dirs.contains(&"proc"));
    }

    #[test]
    fn test_external_path() {
        let mut session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        // Test absolute path
        let path = std::path::Path::new("/tmp/test");
        assert_eq!(
            session.external_path(path),
            session.location().join("tmp/test")
        );
        // Test relative path
        session.chdir(std::path::Path::new("/tmp")).unwrap();
        let path = std::path::Path::new("test");
        assert_eq!(
            session.external_path(path),
            session.location().join("tmp/test")
        );
    }
}
