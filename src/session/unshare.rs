use crate::session::{CommandBuilder, Error, ImageError, Project, Session};
use std::path::{Path, PathBuf};

/// An unshare based session
pub struct UnshareSession {
    root: PathBuf,
    /// Owns the session root when it is a temporary directory this session
    /// created, and removes it on drop. `None` for a session pointed at a root
    /// it does not own. Held only for its `Drop`.
    _root_dir: Option<RootDir>,
    cwd: PathBuf,
    /// Whether to isolate the network namespace (deny network access).
    ///
    /// Held in a `Cell` so it can be toggled through a shared reference: the
    /// network needs to be turned on and off around individual install steps
    /// while the session is borrowed immutably elsewhere (e.g. by an installer).
    isolate_network: std::cell::Cell<bool>,
}

/// A temporary session root, removed on drop.
///
/// Deliberately not a [`tempfile::TempDir`]. The root is populated by `tar` or
/// `mmdebstrap` running in a user namespace, which leaves files owned by uids
/// from the caller's `/etc/subuid` range. Those are not the caller's uid, and the
/// directories holding them are not writable by the caller, so the plain
/// `remove_dir_all` that `TempDir` performs on drop fails with EPERM -- and
/// `TempDir` discards that error, leaving several hundred MB behind per session.
struct RootDir(PathBuf);

impl RootDir {
    fn new() -> Result<Self, crate::session::Error> {
        // Only the directory is taken from tempfile; cleanup is ours.
        let td = tempfile::tempdir().map_err(|e| {
            crate::session::Error::SetupFailure("tempdir failed".to_string(), e.to_string())
        })?;
        Ok(Self(td.keep()))
    }

    fn path(&self) -> &Path {
        &self.0
    }
}

impl Drop for RootDir {
    fn drop(&mut self) {
        if let Err(e) = remove_root(&self.0) {
            log::warn!("Failed to remove session root {}: {}", self.0.display(), e);
        }
    }
}

/// Remove a session root, including files owned by mapped subuids.
///
/// Re-entering a user namespace with `--map-auto` maps the caller's whole subuid
/// range, and `--map-root-user` makes the caller root within it, which is enough
/// to unlink files owned by those subuids. `unshare` may be missing (it is only a
/// hard requirement for *running* a session), so fall back to a direct removal,
/// which suffices for roots that happen to be entirely caller-owned.
fn remove_root(root: &Path) -> std::io::Result<()> {
    let status = std::process::Command::new("unshare")
        .arg("--map-auto")
        .arg("--map-root-user")
        .arg("--setuid=0")
        .arg("--setgid=0")
        .arg("--")
        .arg("rm")
        .arg("-rf")
        .arg("--")
        .arg(root)
        .status();

    match status {
        Ok(status) if status.success() => Ok(()),
        Ok(status) => Err(std::io::Error::other(format!(
            "unshare rm -rf {} exited with {}",
            root.display(),
            status
        ))),
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => std::fs::remove_dir_all(root),
        Err(e) => Err(e),
    }
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

/// Ensure the session root has a usable `/dev/shm`.
///
/// minbase roots ship without `/dev/shm`, but scip-clang needs it for its
/// driver/worker shared-memory IPC. Each command runs in a fresh mount
/// namespace mapped as the calling (non-root) user, which holds no
/// CAP_SYS_ADMIN, so an in-namespace `mount -t tmpfs` is not possible. The root
/// is a real directory tree on the host, though, and POSIX `shm_open` is
/// satisfied by a plain writable `/dev/shm` directory, so create one (mode 1777,
/// matching the usual tmpfs permissions).
fn ensure_dev_shm(root: &Path) -> Result<(), crate::session::Error> {
    use std::os::unix::fs::PermissionsExt;

    let shm = root.join("dev").join("shm");
    std::fs::create_dir_all(&shm).map_err(|e| {
        crate::session::Error::SetupFailure("Failed to create /dev/shm".to_string(), e.to_string())
    })?;
    std::fs::set_permissions(&shm, std::fs::Permissions::from_mode(0o1777)).map_err(|e| {
        crate::session::Error::SetupFailure(
            "Failed to set /dev/shm permissions".to_string(),
            e.to_string(),
        )
    })?;
    Ok(())
}

/// Get the path to a cached Debian tarball if it exists
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "bookworm")
///
/// # Returns
/// * `Option<PathBuf>` - Path to the cached tarball if it exists
pub fn cached_debian_tarball_path(suite: &str) -> Result<PathBuf, crate::session::Error> {
    let arch = std::env::consts::ARCH;
    let arch_name = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        other => other,
    };

    // Use ~/.cache/ognibuild/images/ for caching
    let base_cache_dir = dirs::cache_dir()
        .ok_or_else(|| crate::session::Error::ImageError(ImageError::NoCachedImage))?;
    let cache_dir = base_cache_dir.join("ognibuild").join("images");

    let tarball_name = format!("debian-{}-{}.tar.gz", suite, arch_name);
    Ok(cache_dir.join(&tarball_name))
}

impl UnshareSession {
    /// Set whether to isolate the network namespace.
    ///
    /// When true (the default), the session will have no network access.
    /// When false, the session shares the host's network namespace.
    pub fn set_isolate_network(&self, isolate: bool) {
        self.isolate_network.set(isolate);
    }

    /// Create a cached Debian session from a cloud image
    ///
    /// Looks for a cached tarball in ~/.cache/ognibuild/images/debian-{suite}-{arch}.tar.xz
    /// # Arguments
    /// * `suite` - The Debian suite to use (e.g., "sid", "bookworm")
    pub fn cached_debian_session(suite: &str) -> Result<Self, crate::session::Error> {
        let tarball_path = cached_debian_tarball_path(suite)?;
        if !tarball_path.exists() {
            Err(Error::ImageError(ImageError::NoCachedImage))
        } else {
            log::info!(
                "Using cached Debian {} image from: {}",
                suite,
                tarball_path.display()
            );
            Self::from_tarball(&tarball_path)
        }
    }

    /// Create a session from a tarball
    pub fn from_tarball(path: &Path) -> Result<Self, crate::session::Error> {
        let td = RootDir::new()?;

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
            // -p preserves special permission bits (notably the sticky bit on
            // /tmp). Without it tar applies the umask and drops them, leaving
            // /tmp non-sticky and unwritable by apt's _apt sandbox user.
            .arg("xp")
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

        ensure_dev_shm(root)?;

        let root = root.to_path_buf();
        let s = Self {
            root,
            _root_dir: Some(td),
            cwd: std::path::PathBuf::from("/"),
            isolate_network: std::cell::Cell::new(true),
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
        bootstrap_debian_tarball("sid", true, &[])
    }

    /// Verify that the current user has an account in the session
    pub fn ensure_current_user(&self) -> Result<(), crate::session::Error> {
        // Ensure that the current user has an entry in /etc/passwd
        let user = whoami::username().map_err(|e| {
            crate::session::Error::SetupFailure(
                "Failed to get current username".to_string(),
                e.to_string(),
            )
        })?;
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
        ];
        if self.isolate_network.get() {
            ret.push("--net");
        }
        ret.extend([
            "--uts",
            "--ipc",
            "--root",
            self.root.to_str().unwrap(),
            "--wd",
            cwd.unwrap_or(&self.cwd).to_str().unwrap(),
        ]);
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

/// Create a Debian UnshareSession for testing, with fallback options
///
/// This function tries the following in order:
/// 1. If OGNIBUILD_DEBIAN_TEST_TARBALL is set, use that tarball
/// 2. If a cached image exists, use it
/// 3. Otherwise, bootstrap from network using mmdebstrap
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "unstable", "bookworm", "stable")
pub fn create_debian_session_for_testing(
    suite: &str,
    allow_network: bool,
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

    // Try to use cached session first (without downloading if not present)
    match UnshareSession::cached_debian_session(suite) {
        Ok(session) => {
            // cached_debian_session already logged which image is being used.
            return Ok(session);
        }
        Err(Error::ImageError(ImageError::NoCachedImage)) => {
            log::debug!("No cached image available for Debian {}", suite);
            // Continue to next option: bootstrap from network
        }
        Err(Error::ImageError(ImageError::CachedImageNotFound { path })) => {
            log::debug!("Cached image not found at {}", path.display());
            // Continue to next option: bootstrap from network
        }
        Err(e) => return Err(e), // Other errors should propagate
    }

    if !allow_network {
        return Err(Error::ImageError(ImageError::NoCachedImage));
    }

    // Default: bootstrap from network
    log::info!(
        "No cached image found, bootstrapping Debian {} test session from network using mmdebstrap",
        suite
    );
    bootstrap_debian_tarball(suite, true, &[])
}

/// Bootstrap a Debian system using mmdebstrap and create a tarball
///
/// # Arguments
/// * `suite` - The Debian suite to use (e.g., "sid", "unstable", "bookworm", "stable")
/// * `setup_apt_file` - Whether to install and configure apt-file during bootstrap (requires network)
/// * `extra_packages` - Additional packages to install into the bootstrapped image
pub fn bootstrap_debian_tarball(
    suite: &str,
    setup_apt_file: bool,
    extra_packages: &[&str],
) -> Result<UnshareSession, crate::session::Error> {
    let td = RootDir::new()?;
    let root = td.path();

    // Build mmdebstrap command
    let mut cmd = std::process::Command::new("mmdebstrap");
    cmd.current_dir(root)
        .arg("--mode=unshare")
        .arg("--variant=minbase");

    if !extra_packages.is_empty() {
        cmd.arg(format!("--include={}", extra_packages.join(",")));
    }

    // The customizations below need network access to download indexes, so they
    // are gated on the same flag as apt-file setup.
    if setup_apt_file {
        log::info!(
            "Setting up apt-file and the Sources index in bootstrap (this requires network access)"
        );
        cmd.arg("--include=apt-file")
            // Add a deb-src entry and fetch the Sources index. ognibuild's
            // build-dependency tie-breaker counts how often each candidate
            // package is build-depended on across all source packages; without
            // the Sources index every count is zero and ties are broken
            // arbitrarily (e.g. picking the unusable "rustup" over "cargo").
            .arg(format!(
                "--customize-hook=echo 'deb-src http://deb.debian.org/debian/ {} main' >> \"$1\"/etc/apt/sources.list",
                suite
            ))
            .arg("--customize-hook=chroot \"$1\" apt-get update")
            // Download apt-file Contents files (used to map files to packages).
            .arg("--customize-hook=chroot \"$1\" apt-file update")
            // Preserve apt lists (Sources + Contents) for the tie-breaker and
            // apt-file inside the session.
            .arg("--skip=cleanup/apt/lists");
    }

    cmd.arg("--quiet")
        .arg(suite)
        .arg(root)
        .arg("http://deb.debian.org/debian/");

    let status = cmd.status().map_err(|e| {
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

    ensure_dev_shm(root)?;

    let root = root.to_path_buf();
    let s = UnshareSession {
        root,
        _root_dir: Some(td),
        cwd: std::path::PathBuf::from("/"),
        isolate_network: std::cell::Cell::new(true),
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
        fs_extra::dir::copy(path, &export_directory, &options).map_err(|e| {
            crate::session::Error::SetupFailure(
                format!("failed to copy {} into session", path.display()),
                e.to_string(),
            )
        })?;

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

    fn set_isolate_network(&self, isolate: bool) {
        self.isolate_network.set(isolate);
    }

    fn is_network_isolated(&self) -> bool {
        self.isolate_network.get()
    }
}

#[cfg(test)]
lazy_static::lazy_static! {
    // Serializes access to the process-global OGNIBUILD_DEBIAN_TEST_TARBALL
    // environment variable so that tests mutating it cannot race with the shared
    // session initializer (which also reads it).
    pub(crate) static ref TEST_ENV_LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

    // None when no cached image or test tarball is available, so tests can skip
    // gracefully rather than the lazy initializer panicking on first access.
    //
    // The session is extracted once and shared by every test in the binary, which
    // means it has to outlive them all -- and a `lazy_static` is never dropped, so
    // `RootDir` would never get to remove its root. `drop_test_session` below runs
    // at process exit to do it.
    static ref TEST_SESSION: std::sync::Mutex<Option<UnshareSession>> = std::sync::Mutex::new({
        let _guard = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        create_debian_session_for_testing("sid", false).ok()
    });
}

/// Drop the shared test session at exit, so its root gets removed.
#[cfg(test)]
#[ctor::dtor]
fn drop_test_session() {
    let mut guard = TEST_SESSION.lock().unwrap_or_else(|p| p.into_inner());
    guard.take();
}

/// A locked, available [`UnshareSession`] for use in tests.
///
/// Derefs to the session so existing call sites can use it directly. Only
/// constructed when the underlying session is present.
#[cfg(test)]
pub(crate) struct TestSession(std::sync::MutexGuard<'static, Option<UnshareSession>>);

#[cfg(test)]
impl std::ops::Deref for TestSession {
    type Target = UnshareSession;

    fn deref(&self) -> &Self::Target {
        self.0.as_ref().expect("test session present")
    }
}

#[cfg(test)]
impl std::ops::DerefMut for TestSession {
    fn deref_mut(&mut self) -> &mut Self::Target {
        self.0.as_mut().expect("test session present")
    }
}

#[cfg(test)]
pub(crate) fn test_session() -> Option<TestSession> {
    // Don't run tests if we're in github actions (CI environment restrictions)
    if std::env::var("GITHUB_ACTIONS").is_ok() {
        return None;
    }
    // Handle poisoned mutex: if a previous test panicked while holding the lock,
    // we recover the guard to allow tests to continue.
    let guard = match TEST_SESSION.lock() {
        Ok(guard) => guard,
        Err(poisoned) => poisoned.into_inner(),
    };
    // Skip gracefully when no cached image or test tarball is available.
    if guard.is_none() {
        return None;
    }
    Some(TestSession(guard))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A root populated in a user namespace holds files owned by mapped subuids,
    /// which the calling user cannot unlink. Dropping the session has to remove
    /// them anyway; a plain `remove_dir_all` (as `tempfile::TempDir` does) fails
    /// with EPERM and would leave the whole tree behind.
    #[test]
    fn test_drop_removes_subuid_owned_root() {
        if std::env::var("GITHUB_ACTIONS").is_ok() {
            return;
        }

        let td = tempfile::tempdir().unwrap();
        let root = td.keep();

        // Populate the root the way mmdebstrap does: as root inside a user
        // namespace, so the files land owned by a subuid on the host.
        let status = std::process::Command::new("unshare")
            .arg("--map-auto")
            .arg("--map-root-user")
            .arg("--setuid=0")
            .arg("--setgid=0")
            .arg("--")
            .arg("sh")
            .arg("-c")
            .arg(format!(
                "mkdir -p {0}/usr/bin && touch {0}/usr/bin/f && chown -R 1:1 {0}/usr && chmod -R a-w {0}/usr",
                root.display()
            ))
            .status();

        match status {
            Ok(status) if status.success() => {}
            // No unshare, or no subuid range configured: nothing to test here.
            _ => {
                std::fs::remove_dir_all(&root).ok();
                return;
            }
        }

        // Confirm the setup actually reproduces the condition: the caller must
        // not be able to remove this tree by itself, or the test proves nothing.
        assert!(
            std::fs::remove_dir_all(&root).is_err(),
            "expected the subuid-owned root to be unremovable by the calling user"
        );

        std::mem::drop(RootDir(root.clone()));

        assert!(!root.exists(), "session root {} leaked", root.display());
    }

    /// The root has to be removed even when setup fails partway through, which
    /// is where a `mmdebstrap` root -- already populated, already subuid-owned --
    /// used to be abandoned.
    #[test]
    fn test_root_removed_when_setup_fails() {
        let td = RootDir::new().unwrap();
        let root = td.path().to_path_buf();
        std::fs::write(root.join("half-extracted"), "x").unwrap();

        std::mem::drop(td);

        assert!(!root.exists(), "session root {} leaked", root.display());
    }

    #[test]
    fn test_drop_leaves_unowned_root_alone() {
        let td = tempfile::tempdir().unwrap();
        let session = UnshareSession {
            root: td.path().to_path_buf(),
            _root_dir: None,
            cwd: std::path::PathBuf::from("/"),
            isolate_network: std::cell::Cell::new(true),
        };
        std::mem::drop(session);
        assert!(td.path().exists());
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
    fn test_dev_shm_writable() {
        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };
        // scip-clang relies on a writable /dev/shm as the calling user; verify a
        // command run without an explicit user can create a file there.
        session
            .check_call(
                vec!["touch", "/dev/shm/ognibuild-test"],
                Some(std::path::Path::new("/")),
                None,
                None,
            )
            .unwrap();
        session
            .check_call(
                vec!["rm", "/dev/shm/ognibuild-test"],
                Some(std::path::Path::new("/")),
                None,
                None,
            )
            .unwrap();
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
        let result = UnshareSession::cached_debian_session("test-suite-nonexistent");
        assert!(result.is_err());
        if let Err(err) = result {
            assert!(
                matches!(
                    err,
                    crate::session::Error::ImageError(
                        crate::session::ImageError::CachedImageNotFound { .. }
                            | crate::session::ImageError::NoCachedImage
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

        let result = UnshareSession::cached_debian_session("sid");
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
        let _env_guard = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
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
        let result = create_debian_session_for_testing("sid", false);

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
        let _env_guard = TEST_ENV_LOCK.lock().unwrap_or_else(|p| p.into_inner());
        std::env::set_var(
            "OGNIBUILD_DEBIAN_TEST_TARBALL",
            "/nonexistent/path/tarball.tar.xz",
        );

        let result = create_debian_session_for_testing("sid", false);

        std::env::remove_var("OGNIBUILD_DEBIAN_TEST_TARBALL");

        assert!(result.is_err());
        if let Err(err) = result {
            // Should be a SetupFailure about non-existent file
            match err {
                crate::session::Error::SetupFailure(_msg, detail) => {
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
        let result = UnshareSession::cached_debian_session("sid");

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
    fn test_set_isolate_network() {
        let session = UnshareSession {
            root: std::path::PathBuf::from("/fakechroot"),
            _root_dir: None,
            cwd: std::path::PathBuf::from("/"),
            isolate_network: std::cell::Cell::new(true),
        };
        let argv = session.run_argv(vec!["true"], Some(std::path::Path::new("/")), None);
        assert!(argv.contains(&"--net"));

        session.set_isolate_network(false);
        let argv = session.run_argv(vec!["true"], Some(std::path::Path::new("/")), None);
        assert!(!argv.contains(&"--net"));

        session.set_isolate_network(true);
        let argv = session.run_argv(vec!["true"], Some(std::path::Path::new("/")), None);
        assert!(argv.contains(&"--net"));
    }

    #[test]
    fn test_with_network_restores_isolation() {
        let session = UnshareSession {
            root: std::path::PathBuf::from("/fakechroot"),
            _root_dir: None,
            cwd: std::path::PathBuf::from("/"),
            isolate_network: std::cell::Cell::new(true),
        };

        let inside = crate::session::with_network(&session, || session.isolate_network.get());
        assert!(!inside);
        assert!(session.isolate_network.get());
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
