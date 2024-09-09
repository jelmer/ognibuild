use crate::session::{CommandBuilder, Error, Project, Session};

pub struct PlainSession(std::path::PathBuf);

impl Default for PlainSession {
    fn default() -> Self {
        Self::new()
    }
}

impl PlainSession {
    pub fn new() -> Self {
        PlainSession(std::path::PathBuf::from("/"))
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
        self.0.join(path).exists()
    }

    fn mkdir(&self, path: &std::path::Path) -> Result<(), Error> {
        std::fs::create_dir_all(self.0.join(path)).map_err(Error::IoError)
    }

    fn chdir(&mut self, path: &std::path::Path) -> Result<(), Error> {
        self.0 = self.0.join(path);
        Ok(())
    }

    fn pwd(&self) -> &std::path::Path {
        &self.0
    }

    fn external_path(&self, path: &std::path::Path) -> std::path::PathBuf {
        self.0.join(path).canonicalize().unwrap()
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

        cmd = cmd.current_dir(cwd.unwrap_or(self.0.as_path()));

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        let output = cmd.output();

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

        cmd = cmd.current_dir(cwd.unwrap_or(self.0.as_path()));

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        let status = cmd.status();

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

    fn create_home(&self) -> Result<(), Error> {
        Ok(())
    }

    fn project_from_directory(
        &self,
        path: &std::path::Path,
        _subdir: Option<&str>,
    ) -> Result<Project, Error> {
        Ok(Project::Noop(path.to_path_buf()))
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
    ) -> std::process::Child {
        let argv = self.prepend_user(user, argv);

        let mut binding = std::process::Command::new(argv[0]);

        let mut cmd = binding
            .args(&argv[1..])
            .stdin(stdin.unwrap_or(std::process::Stdio::inherit()))
            .stdout(stdout.unwrap_or(std::process::Stdio::inherit()))
            .stderr(stderr.unwrap_or(std::process::Stdio::inherit()));

        let cwd = cwd.map_or_else(|| self.0.clone(), |p| self.0.join(p));
        cmd = cmd.current_dir(cwd);

        if let Some(env) = env {
            cmd = cmd.envs(env);
        }

        cmd.spawn().unwrap()
    }

    fn is_temporary(&self) -> bool {
        false
    }

    #[cfg(feature = "breezy")]
    fn project_from_vcs(
        &self,
        tree: &dyn crate::vcs::DupableTree,
        include_controldir: Option<bool>,
        subdir: Option<&str>,
    ) -> Result<Project, Error> {
        use crate::vcs::{dupe_vcs_tree, export_vcs_tree};
        if include_controldir.unwrap_or(true) && tree.basedir().is_some() {
            // Optimization: just use the directory as-is, don't copy anything
            Ok(Project::Noop(tree.basedir().unwrap()))
        } else if !include_controldir.unwrap_or(false) {
            let td = tempfile::tempdir().unwrap();
            let p = if let Some(subdir) = subdir {
                td.path().join(subdir)
            } else {
                td.path().to_path_buf()
            };
            export_vcs_tree(tree.as_tree(), &p, None).unwrap();
            Ok(Project::Temporary {
                internal_path: p.clone(),
                external_path: p,
                td: td.into_path(),
            })
        } else {
            let td = tempfile::tempdir().unwrap();
            let p = if let Some(subdir) = subdir {
                td.path().join(subdir)
            } else {
                td.path().to_path_buf()
            };
            dupe_vcs_tree(tree, &p).unwrap();
            Ok(Project::Temporary {
                internal_path: p.clone(),
                external_path: p,
                td: td.into_path(),
            })
        }
    }

    fn command<'a>(&'a self, argv: Vec<&'a str>) -> CommandBuilder<'a> {
        CommandBuilder::new(self, argv)
    }

    fn read_dir(&self, path: &std::path::Path) -> Result<Vec<std::fs::DirEntry>, Error> {
        std::fs::read_dir(path)
            .map_err(Error::IoError)?
            .collect::<Result<Vec<_>, _>>()
            .map_err(Error::IoError)
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
    fn test_is_temporary() {
        let session = PlainSession::new();
        assert!(!session.is_temporary());
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
        let pwd_bytes = session.check_output(vec!["pwd"], None, None, None).unwrap();
        let reported =
            std::str::from_utf8(pwd_bytes.as_slice().strip_suffix(b"\n").unwrap()).unwrap();
        assert_eq!(reported, path.canonicalize().unwrap().to_str().unwrap());
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
    fn test_project_from_directory() {
        let session = PlainSession::new();
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test");
        session.mkdir(&path).unwrap();
        let project = session.project_from_directory(&path, None).unwrap();
        assert_eq!(project.external_path(), path);
        assert_eq!(project.internal_path(), path);
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

    #[cfg(feature = "breezy")]
    #[test]
    fn test_project_from_vcs() {
        use breezyshim::tree::MutableTree;
        let env = breezyshim::testing::TestEnv::new();
        let session = PlainSession::new();

        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();

        let path = td.path();

        tree.put_file_bytes_non_atomic(std::path::Path::new("test"), b"hello")
            .unwrap();
        tree.add(&[std::path::Path::new("test")]).unwrap();
        tree.build_commit().message("test").commit().unwrap();
        let project = session.project_from_vcs(&tree, None, None).unwrap();
        assert_eq!(project.external_path(), path);
        assert_eq!(project.internal_path(), path);
        assert!(project.external_path().join(".bzr").exists());

        let project = session.project_from_vcs(&tree, Some(true), None).unwrap();
        assert_eq!(project.external_path(), path);
        assert_eq!(project.internal_path(), path);

        assert!(project.external_path().join(".bzr").exists());

        let project = session.project_from_vcs(&tree, Some(false), None).unwrap();
        assert_ne!(project.external_path(), path);
        assert_ne!(project.internal_path(), path);

        assert!(!project.external_path().join(".bzr").exists());
        std::mem::drop(env);
    }

    #[test]
    fn test_output() {
        let session = PlainSession::new();
        let output = session
            .command(vec!["echo", "hello"])
            .stdout(std::process::Stdio::piped())
            .output()
            .unwrap()
            .stdout;
        assert_eq!(output, b"hello\n");
    }
}
