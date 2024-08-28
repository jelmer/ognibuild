use crate::session::{CommandBuilder, Error, Session};

pub struct UnshareSession {
    root: std::path::PathBuf,
    _tempdir: Option<tempfile::TempDir>,
    cwd: Option<std::path::PathBuf>,
}

impl UnshareSession {
    pub fn bootstrap() -> Result<Self, crate::session::Error> {
        let td = tempfile::tempdir().map_err(|e| {
            crate::session::Error::SetupFailure("tempdir failed".to_string(), e.to_string())
        })?;

        let root = td.path();
        std::process::Command::new("mmdebstrap")
            .current_dir(root)
            .arg("--variant=minbase")
            .arg("--quiet")
            .arg("sid")
            .arg(root)
            .arg("http://deb.debian.org/debian/")
            .status()
            .map_err(|e| {
                crate::session::Error::SetupFailure("debootstrap failed".to_string(), e.to_string())
            })?;

        let s = Self {
            root: root.to_path_buf(),
            _tempdir: Some(td),
            cwd: None,
        };

        s.ensure_current_user();

        Ok(s)
    }

    pub fn ensure_current_user(&self) {
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

        match self.check_call(
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
        ) {
            Ok(_) => {}
            // Ignore if user already exists
            Err(Error::CalledProcessError(status)) if status.code() == Some(9) || status.code() == Some(4) => {}
            Err(e) => panic!("Error: {:?}", e),
        }
    }

    pub fn run_argv<'a>(
        &'a self,
        argv: Vec<&'a str>,
        cwd: Option<&'a std::path::Path>,
        user: Option<&'a str>,
    ) -> std::vec::Vec<&'a str> {
        let mut ret = vec![
            "unshare",
            "--map-auto",
            "--fork",
            "--pid",
            "--mount-proc",
            "--net",
            "--uts",
            "--ipc",
            "--root",
            self.root.to_str().unwrap(),
            "--wd",
            cwd.or(self.cwd.as_deref())
                .map(|x| x.to_str().unwrap())
                .unwrap_or("/"),
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
        self.cwd = Some(path.to_path_buf());
        Ok(())
    }

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        if let Ok(rest) = path.strip_prefix("/") {
            return self.location().join(rest);
        }
        if let Some(cwd) = &self.cwd {
            return self
                .location()
                .join(cwd.to_string_lossy().to_string().trim_start_matches('/'))
                .join(path.to_string_lossy().to_string().trim_start_matches('/'));
        } else {
            panic!("no cwd set");
        }
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

    fn setup_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), super::Error> {
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

        Ok((export_directory, reldir.join(subdir)))
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
        let argv = self.run_argv(argv, cwd, user);

        std::process::Command::new(argv[0])
            .args(&argv[1..])
            .envs(env.unwrap_or_default())
            .stdin(stdin.unwrap_or(std::process::Stdio::inherit()))
            .stdout(stdout.unwrap_or(std::process::Stdio::inherit()))
            .stderr(stderr.unwrap_or(std::process::Stdio::inherit()))
            .spawn()
            .unwrap()
    }

    fn is_temporary(&self) -> bool {
        true
    }

    fn setup_from_vcs(
        &self,
        tree: &dyn crate::vcs::DupableTree,
        include_controldir: Option<bool>,
        subdir: Option<&std::path::Path>,
    ) -> Result<(std::path::PathBuf, std::path::PathBuf), Error> {
        let reldir = self.build_tempdir(None);

        let subdir = subdir.unwrap_or_else(|| std::path::Path::new("package"));

        let export_directory = self.external_path(&reldir).join(subdir);
        if !include_controldir.unwrap_or(false) {
            crate::vcs::export_vcs_tree(tree.as_tree(), &export_directory, Some(subdir)).unwrap();
        } else {
            crate::vcs::dupe_vcs_tree(tree, &export_directory).unwrap();
        }

        Ok((export_directory, reldir.join(subdir)))
    }

    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a> {
        CommandBuilder::new(self, argv)
    }

    fn read_dir(&self, path: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, Error> {
        std::fs::read_dir(self.external_path(path)).map_err(Error::IoError)?.collect::<Result<Vec<_>, _>>().map_err(Error::IoError)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    lazy_static::lazy_static! {
        static ref TEST_SESSION: std::sync::Mutex<UnshareSession> = std::sync::Mutex::new(UnshareSession::bootstrap().unwrap());
    }

    fn test_session() -> std::sync::MutexGuard<'static, UnshareSession> {
        TEST_SESSION.lock().unwrap()
    }

    #[test]
    fn test_is_temporary() {
        let session = test_session();
        assert!(session.is_temporary());
    }

    #[test]
    fn test_chdir() {
        let mut session = test_session();
        session.chdir(std::path::Path::new("/")).unwrap();
    }

    #[test]
    fn test_check_output() {
        let session = test_session();
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
        let session = test_session();
        session
            .check_call(vec!["true"], Some(std::path::Path::new("/")), None, None)
            .unwrap();
    }

    #[test]
    fn test_create_home() {
        let session = test_session();
        session.create_home().unwrap();
    }

    #[test]
    fn test_mkdir_rmdir() {
        let session = test_session();
        let path = std::path::Path::new("/tmp/test");
        session.mkdir(path).unwrap();
        assert!(session.exists(path));
        session.rmtree(path).unwrap();
        assert!(!session.exists(path));
    }

    #[test]
    fn test_setup_from_directory() {
        let session = test_session();
        let tempdir = tempfile::tempdir().unwrap();
        std::fs::write(tempdir.path().join("test"), "test").unwrap();
        let (export_directory, subdir) =
            session.setup_from_directory(tempdir.path(), None).unwrap();
        assert!(export_directory.exists());
        assert!(session.exists(&subdir));
        session.rmtree(&subdir).unwrap();
        assert!(!session.exists(&subdir));
        assert!(!export_directory.exists());
    }

    #[test]
    fn test_popen() {
        let session = test_session();
        let child = session.popen(
            vec!["ls"],
            Some(std::path::Path::new("/")),
            None,
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
            Some(std::process::Stdio::piped()),
            None,
        );
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
        let mut session = test_session();
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
