use crate::session::{CommandBuilder, Error, ImageError, Project, Session};
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
    /// Create a cached Debian session from a cloud image
    ///
    /// Looks for a cached tarball in ~/.cache/ognibuild/images/debian-{suite}-{arch}.tar.xz
    /// If not found and allow_download is true, downloads it from cdimage.debian.org (requires 'debian' feature)
    ///
    /// # Arguments
    /// * `suite` - The Debian suite to use (e.g., "sid", "bookworm")
    /// * `allow_download` - Whether to download the image if not cached (requires 'debian' feature)
    pub fn cached_debian_session(
        suite: &str,
        allow_download: bool,
    ) -> Result<Self, crate::session::Error> {
        let arch = std::env::consts::ARCH;
        let arch_name = match arch {
            "x86_64" => "amd64",
            "aarch64" => "arm64",
            _ => {
                return Err(Error::ImageError(ImageError::UnsupportedArchitecture {
                    arch: arch.to_string(),
                }))
            }
        };

        // Use ~/.cache/ognibuild/images/ for caching
        let cache_dir = dirs::cache_dir()
            .ok_or_else(|| {
                Error::SetupFailure(
                    "Cannot determine cache directory".to_string(),
                    "Unable to find user cache directory".to_string(),
                )
            })?
            .join("ognibuild")
            .join("images");

        std::fs::create_dir_all(&cache_dir).map_err(|e| {
            Error::SetupFailure("Failed to create cache dir".to_string(), e.to_string())
        })?;

        let tarball_name = format!("debian-{}-{}.tar.xz", suite, arch_name);
        let tarball_path = cache_dir.join(&tarball_name);

        // Check if already cached
        if !tarball_path.exists() {
            if !allow_download {
                return Err(Error::ImageError(ImageError::CachedImageNotFound {
                    path: tarball_path,
                }));
            }

            #[cfg(feature = "debian")]
            {
                log::info!("Cached Debian {} image not found, downloading...", suite);
                download_debian_cloud_image(suite, &tarball_path)?;
            }

            #[cfg(not(feature = "debian"))]
            {
                return Err(Error::ImageError(ImageError::DownloadNotAvailable {
                    reason: "Downloading cloud images requires the 'debian' feature to be enabled"
                        .to_string(),
                }));
            }
        } else {
            log::info!(
                "Using cached Debian {} image from: {}",
                suite,
                tarball_path.display()
            );
        }

        Self::from_tarball(&tarball_path)
    }

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

        // Create necessary directories for mounting before extraction
        // These might not exist in cloud images
        for dir in &["proc", "sys", "dev"] {
            std::fs::create_dir_all(root.join(dir)).map_err(|e| {
                crate::session::Error::SetupFailure(
                    format!("Failed to create {} directory", dir),
                    e.to_string(),
                )
            })?;
        }

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

    /// Bootstrap the session environment with Debian sid
    pub fn bootstrap() -> Result<Self, crate::session::Error> {
        bootstrap_debian_tarball("sid")
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

/// Download a Debian cloud image tarball
///
/// Downloads from cdimage.debian.org to the specified path
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "bookworm")
/// * `tarball_path` - Path where the tarball should be saved
#[cfg(feature = "debian")]
pub fn download_debian_cloud_image(
    suite: &str,
    tarball_path: &Path,
) -> Result<(), crate::session::Error> {
    let arch = std::env::consts::ARCH;
    let arch_name = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => {
            return Err(Error::ImageError(ImageError::UnsupportedArchitecture {
                arch: arch.to_string(),
            }))
        }
    };

    let tarball_name = format!("debian-{}-generic-{}-daily.tar.xz", suite, arch_name);
    let url = format!(
        "https://cdimage.debian.org/images/cloud/{}/daily/latest/{}",
        suite, tarball_name
    );

    log::info!("Downloading Debian {} cloud image from {}...", suite, url);

    // Download the file using reqwest blocking client
    let client = reqwest::blocking::Client::new();
    let mut response = client.get(&url).send().map_err(|e| {
        Error::ImageError(ImageError::DownloadFailed {
            url: url.clone(),
            error: e.to_string(),
        })
    })?;

    if !response.status().is_success() {
        return Err(Error::ImageError(ImageError::DownloadFailed {
            url: url.clone(),
            error: format!("Server returned status {}", response.status()),
        }));
    }

    let mut file = std::fs::File::create(tarball_path).map_err(|e| {
        Error::SetupFailure("Failed to create tarball file".to_string(), e.to_string())
    })?;

    std::io::copy(&mut response, &mut file)
        .map_err(|e| Error::SetupFailure("Failed to write tarball".to_string(), e.to_string()))?;

    log::info!(
        "Debian {} cloud image downloaded to: {}",
        suite,
        tarball_path.display()
    );
    Ok(())
}

/// Create a Debian UnshareSession for testing, with fallback options
///
/// This function tries the following in order:
/// 1. If OGNIBUILD_DEBIAN_TEST_TARBALL is set, use that tarball
/// 2. If OGNIBUILD_USE_DEBIAN_CLOUD_IMAGE is set, use cached cloud image
/// 3. Otherwise, bootstrap from network using mmdebstrap
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "unstable", "bookworm", "stable")
pub fn create_debian_session_for_testing(
    suite: &str,
) -> Result<UnshareSession, crate::session::Error> {
    // Check if a custom tarball path is provided for testing
    if let Ok(tarball_path) = std::env::var("OGNIBUILD_DEBIAN_TEST_TARBALL") {
        let path = Path::new(&tarball_path);
        if path.exists() {
            log::info!(
                "Using Debian test tarball from OGNIBUILD_DEBIAN_TEST_TARBALL: {}",
                tarball_path
            );
            return UnshareSession::from_tarball(path);
        } else {
            return Err(Error::SetupFailure(
                "Tarball not found".to_string(),
                format!(
                    "OGNIBUILD_DEBIAN_TEST_TARBALL points to non-existent file: {}",
                    tarball_path
                ),
            ));
        }
    }

    // Check if we should use a cached Debian cloud image for testing
    if std::env::var("OGNIBUILD_USE_DEBIAN_CLOUD_IMAGE").is_ok() {
        log::info!(
            "OGNIBUILD_USE_DEBIAN_CLOUD_IMAGE is set, attempting to use cached Debian {} image",
            suite
        );
        return UnshareSession::cached_debian_session(suite, true);
    }

    // Default: bootstrap from network
    log::info!(
        "Bootstrapping Debian {} test session from network using mmdebstrap",
        suite
    );
    bootstrap_debian_tarball(suite)
}

/// Bootstrap a Debian system using mmdebstrap and create a tarball
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "unstable", "bookworm", "stable")
pub fn bootstrap_debian_tarball(suite: &str) -> Result<UnshareSession, crate::session::Error> {
    let td = tempfile::tempdir().map_err(|e| {
        crate::session::Error::SetupFailure("tempdir failed".to_string(), e.to_string())
    })?;

    let root = td.path();
    let status = std::process::Command::new("mmdebstrap")
        .current_dir(root)
        .arg("--mode=unshare")
        .arg("--variant=minbase")
        .arg("--quiet")
        .arg(suite)
        .arg(root)
        .arg("http://deb.debian.org/debian/")
        .status()
        .map_err(|e| {
            crate::session::Error::SetupFailure(
                "mmdebstrap command not found or failed to execute".to_string(),
                format!("Failed to run mmdebstrap (ensure it's installed): {}", e),
            )
        })?;

    if !status.success() {
        return Err(crate::session::Error::SetupFailure(
            "mmdebstrap failed".to_string(),
            format!("mmdebstrap exited with status: {}. This likely requires network access to http://deb.debian.org/debian/", status),
        ));
    }

    let s = UnshareSession {
        root: root.to_path_buf(),
        _tempdir: Some(td),
        cwd: std::path::PathBuf::from("/"),
    };

    s.ensure_current_user()?;

    Ok(s)
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
        static ref TEST_SESSION: std::sync::Mutex<UnshareSession> = std::sync::Mutex::new(
            create_debian_session_for_testing("sid")
                .expect("Failed to create test session. This requires network access.\nYou can avoid this by setting:\n  OGNIBUILD_DEBIAN_TEST_TARBALL=/path/to/tarball.tar.xz\n  OGNIBUILD_USE_DEBIAN_CLOUD_IMAGE=1 (downloads once from cdimage.debian.org)")
        );
    }

    fn test_session() -> Option<std::sync::MutexGuard<'static, UnshareSession>> {
        // Don't run tests if we're in github actions (CI environment restrictions)
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            return None;
        }
        // Handle poisoned mutex: if a previous test panicked while holding the lock,
        // we recover the guard to allow tests to continue
        match TEST_SESSION.lock() {
            Ok(guard) => Some(guard),
            Err(poisoned) => {
                // Recover from poisoned mutex - this is safe because UnshareSession
                // doesn't have invalid states that could cause issues after a panic
                Some(poisoned.into_inner())
            }
        }
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
    fn test_session_works_after_panic() {
        // Skip if we're in CI
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            return;
        }

        // First, verify we can get the session normally
        let session1 = test_session().unwrap();
        assert!(session1.exists(std::path::Path::new("/bin")));
        std::mem::drop(session1);

        // Now cause a panic while holding the lock
        let result = std::panic::catch_unwind(|| {
            let _session = test_session().unwrap();
            panic!("Intentional panic to test recovery");
        });

        // Verify the panic happened
        assert!(result.is_err());

        // Now verify we can still get the session (it shouldn't be blocked)
        let session2 = test_session().unwrap();
        assert!(session2.exists(std::path::Path::new("/bin")));

        // Verify the session is still functional by running a command
        session2
            .check_call(vec!["true"], Some(std::path::Path::new("/")), None, None)
            .unwrap();
    }

    #[test]
    fn test_cached_debian_session_no_download() {
        // Test that cached_debian_session returns the correct error when download is not allowed
        // and no cached file exists
        let result = UnshareSession::cached_debian_session("test-suite-nonexistent", false);
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(
                matches!(
                    err,
                    crate::session::Error::ImageError(
                        crate::session::ImageError::CachedImageNotFound { .. }
                    )
                ),
                "Expected CachedImageNotFound error, got {:?}",
                err
            );
        }
    }

    #[test]
    fn test_cached_debian_session_unsupported_arch() {
        // This test will only work on architectures that are not x86_64 or aarch64
        let arch = std::env::consts::ARCH;
        if arch == "x86_64" || arch == "aarch64" {
            // Skip this test on supported architectures
            return;
        }

        let result = UnshareSession::cached_debian_session("sid", false);
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(
                matches!(
                    err,
                    crate::session::Error::ImageError(
                        crate::session::ImageError::UnsupportedArchitecture { .. }
                    )
                ),
                "Expected UnsupportedArchitecture error, got {:?}",
                err
            );
        }
    }

    #[test]
    fn test_create_debian_session_with_env_var() {
        // Test that create_debian_session_for_testing respects OGNIBUILD_DEBIAN_TEST_TARBALL
        let temp_dir = tempfile::tempdir().unwrap();
        let tarball_path = temp_dir.path().join("test.tar.xz");

        // Create a minimal test tarball (invalid but exists)
        std::fs::write(&tarball_path, b"test").unwrap();

        // Set the environment variable to use this tarball
        std::env::set_var(
            "OGNIBUILD_DEBIAN_TEST_TARBALL",
            tarball_path.to_str().unwrap(),
        );

        // This should attempt to use the tarball (will fail because it's not valid, but that's ok)
        let result = create_debian_session_for_testing("sid");

        // Clean up
        std::env::remove_var("OGNIBUILD_DEBIAN_TEST_TARBALL");

        // We expect this to fail because our test tarball is not valid,
        // but it should fail in from_tarball with a SetupFailure, not because the file doesn't exist
        assert!(result.is_err());
        if let Err(err) = result {
            // Should be a SetupFailure from tar extraction, not a file not found error
            assert!(
                matches!(err, crate::session::Error::SetupFailure(_, _)),
                "Expected SetupFailure from tar extraction, got {:?}",
                err
            );
        }
    }

    #[test]
    fn test_create_debian_session_nonexistent_tarball() {
        // Test that pointing to a non-existent tarball gives the right error
        std::env::set_var(
            "OGNIBUILD_DEBIAN_TEST_TARBALL",
            "/nonexistent/path/tarball.tar.xz",
        );

        let result = create_debian_session_for_testing("sid");

        std::env::remove_var("OGNIBUILD_DEBIAN_TEST_TARBALL");

        assert!(result.is_err());
        if let Err(err) = result {
            // Should be a SetupFailure about non-existent file
            match err {
                crate::session::Error::SetupFailure(msg, detail) => {
                    assert!(
                        detail.contains("non-existent file"),
                        "Expected error about non-existent file, got: {}",
                        detail
                    );
                }
                _ => panic!("Expected SetupFailure, got {:?}", err),
            }
        }
    }

    #[cfg(not(feature = "debian"))]
    #[test]
    fn test_cached_debian_session_no_debian_feature() {
        // When debian feature is not enabled, downloading should return DownloadNotAvailable error
        let result = UnshareSession::cached_debian_session("sid", true);

        // If the cache doesn't exist, it should fail with DownloadNotAvailable
        // (assuming the cache doesn't exist for this test)
        if result.is_err() {
            if let Err(err) = result {
                // Could be CachedImageNotFound if cache exists, or DownloadNotAvailable if trying to download
                assert!(
                    matches!(err, crate::session::Error::ImageError(_)),
                    "Expected ImageError, got {:?}",
                    err
                );
            }
        }
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
