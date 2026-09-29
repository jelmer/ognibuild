use crate::session::{CommandBuilder, Error, Project, Session};
use std::io::{BufRead, Read};

extern crate rand;
use rand::distr::{Alphanumeric, Distribution};
use std::iter;

/// Number of random characters appended to a generated session name.
const RANDOM_SUFFIX_LEN: usize = 8;

/// Sanitize the session name
pub fn sanitize_session_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c.is_alphanumeric() || "_-.".contains(c))
        .collect()
}

/// Generate a session name of the form `<sanitized prefix>-<pid>-<random>`,
/// where the pid is the process that owns the session.
pub fn generate_session_id(prefix: &str) -> String {
    let mut rng = rand::rng();
    let suffix: String = String::from_utf8(
        iter::repeat(())
            .map(|()| Alphanumeric.sample(&mut rng))
            .take(RANDOM_SUFFIX_LEN)
            .collect(),
    )
    .unwrap();
    format!(
        "{}-{}-{}",
        sanitize_session_name(prefix),
        std::process::id(),
        suffix
    )
}

/// Owning pid of `session_name`, if `generate_session_id` could have produced
/// it from `sanitized_prefix`. Requiring the whole remainder anchors the match.
fn session_name_owner_pid(session_name: &str, sanitized_prefix: &str) -> Option<libc::pid_t> {
    let rest = session_name
        .strip_prefix(sanitized_prefix)?
        .strip_prefix('-')?;
    let (pid, random) = rest.split_once('-')?;
    if random.len() != RANDOM_SUFFIX_LEN || !random.chars().all(|c| c.is_ascii_alphanumeric()) {
        return None;
    }
    if pid.is_empty() || !pid.chars().all(|c| c.is_ascii_digit()) {
        return None;
    }
    let pid: libc::pid_t = pid.parse().ok()?;
    if pid <= 0 {
        return None;
    }
    Some(pid)
}

/// Whether a process with this pid exists, whoever owns it. `EPERM` means it
/// exists and is somebody else's, so only `ESRCH` counts as gone.
fn pid_is_alive(pid: libc::pid_t) -> bool {
    if unsafe { libc::kill(pid, 0) } == 0 {
        return true;
    }
    std::io::Error::last_os_error().raw_os_error() != Some(libc::ESRCH)
}

/// Session names in `schroot --list --all-sessions` output that were generated
/// from `prefix` and whose owning process is gone.
fn stale_session_names(stdout: &str, prefix: &str) -> Result<Vec<String>, Error> {
    let sanitized = sanitize_session_name(prefix);
    if sanitized.is_empty() {
        return Err(Error::InvalidSessionPrefix(prefix.to_string()));
    }
    Ok(filter_stale_session_names(stdout, &sanitized, pid_is_alive))
}

fn filter_stale_session_names(
    stdout: &str,
    sanitized_prefix: &str,
    is_alive: impl Fn(libc::pid_t) -> bool,
) -> Vec<String> {
    let mut names = vec![];
    for line in stdout.lines() {
        let line = line.trim();
        let name = line.strip_prefix("session:").unwrap_or(line);
        let pid = match session_name_owner_pid(name, sanitized_prefix) {
            Some(pid) => pid,
            None => continue,
        };
        if is_alive(pid) {
            log::debug!("Leaving schroot session {} to live pid {}", name, pid);
            continue;
        }
        names.push(name.to_string());
    }
    names
}

/// What `purge_stale_sessions` did.
#[derive(Debug, Default, Clone, PartialEq, Eq)]
pub struct PurgeReport {
    /// Sessions that were ended.
    pub purged: Vec<String>,
    /// Sessions that should have been ended, each with the reason it was not.
    pub failed: Vec<(String, String)>,
}

/// End every schroot session on the host named `<prefix>-<pid>-<random>` by
/// `generate_session_id` whose pid is no longer running, whichever user owns it.
pub fn purge_stale_sessions(prefix: &str) -> Result<PurgeReport, Error> {
    let output = std::process::Command::new("schroot")
        .args(["--list", "--all-sessions"])
        .output()
        .map_err(|e| Error::from_spawn("schroot", e))?;
    if !output.status.success() {
        return Err(Error::CalledProcessError(output.status));
    }

    let mut report = PurgeReport::default();
    for name in stale_session_names(&String::from_utf8_lossy(&output.stdout), prefix)? {
        match std::process::Command::new("schroot")
            .arg("-c")
            .arg(format!("session:{}", name))
            .arg("-e")
            .output()
        {
            Ok(o) if o.status.success() => {
                log::info!("Purged stale schroot session {}", name);
                report.purged.push(name);
            }
            Ok(o) => {
                log::error!(
                    "Failed to purge stale schroot session {} (exit {})",
                    name,
                    o.status
                );
                report.failed.push((name, format!("exit {}", o.status)));
            }
            Err(e) => {
                log::error!("Failed to purge stale schroot session {}: {}", name, e);
                report.failed.push((name, e.to_string()));
            }
        }
    }
    Ok(report)
}

/// A schroot-based session
pub struct SchrootSession {
    cwd: std::path::PathBuf,
    session_id: String,
    location: std::path::PathBuf,
}

impl SchrootSession {
    /// Create a schroot session
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
            .map_err(|e| Error::from_spawn("schroot", e))?;

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
            .map_err(|e| Error::from_spawn("schroot", e))?;
        let location = std::path::PathBuf::from(
            String::from_utf8(output.stdout)
                .unwrap()
                .trim_end_matches('\n'),
        );

        Ok(Self {
            cwd: std::path::PathBuf::from("/"),
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
        let cwd = cwd.unwrap_or(self.pwd());

        base_argv.extend([
            "-d".to_string(),
            cwd.to_path_buf().to_string_lossy().to_string(),
        ]);

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
        let log_stderr = |stderr: &std::fs::File| {
            for line in std::io::BufReader::new(stderr).lines() {
                let line = line.unwrap();
                if let Some(rest) = line.strip_prefix("E: ") {
                    log::error!("{}", rest);
                }
            }
        };
        match std::process::Command::new("schroot")
            .arg("-c")
            .arg(format!("session:{}", self.session_id))
            .arg("-e")
            .stderr(std::process::Stdio::from(stderr.try_clone().unwrap()))
            .output()
        {
            Err(_) => {
                log_stderr(&stderr);
                log::error!(
                    "Failed to close schroot session {}, leaving stray.",
                    self.session_id
                );
            }
            Ok(output) if !output.status.success() => {
                log_stderr(&stderr);
                log::error!(
                    "Failed to close schroot session {} (exit {}), leaving stray.",
                    self.session_id,
                    output.status
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
        if let Some(rest) = path.strip_prefix('/') {
            return self.location().join(rest);
        }

        self.location()
            .join(
                self.cwd
                    .to_string_lossy()
                    .to_string()
                    .trim_start_matches('/'),
            )
            .join(path.as_ref())
    }

    fn location(&self) -> std::path::PathBuf {
        self.location.clone()
    }

    fn exists(&self, path: &std::path::Path) -> bool {
        let fullpath = self.external_path(path);
        fullpath.exists()
    }

    fn chdir(&mut self, path: &std::path::Path) -> Result<(), Error> {
        self.cwd = self.cwd.join(path);
        Ok(())
    }

    fn pwd(&self) -> &std::path::Path {
        &self.cwd
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
            .stderr(std::process::Stdio::inherit())
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
                    Err(Error::CalledProcessError(status))
                }
            }
            Err(e) => Err(Error::IoError(e)),
        }
    }

    fn create_home(&self) -> Result<(), Error> {
        crate::session::create_home(self)
    }

    fn project_from_directory(
        &self,
        path: &std::path::Path,
        subdir: Option<&str>,
    ) -> Result<Project, Error> {
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
        fs_extra::dir::copy(path, &export_directory, &options).map_err(|e| {
            Error::SetupFailure(
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
        let argv = self.run_argv(argv, cwd, user, env);

        Ok(std::process::Command::new(&argv[0])
            .args(&argv[1..])
            .stdin(stdin.unwrap_or(std::process::Stdio::inherit()))
            .stdout(stdout.unwrap_or(std::process::Stdio::inherit()))
            .stderr(stderr.unwrap_or(std::process::Stdio::inherit()))
            .spawn()?)
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
        let reldir = self.build_tempdir();

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
    fn test_session_name_owner_pid() {
        assert_eq!(
            super::session_name_owner_pid("janitor-worker-4242-abcd1234", "janitor-worker"),
            Some(4242)
        );
        assert_eq!(
            super::session_name_owner_pid("other-tool-4242-abcd1234", "janitor-worker"),
            None
        );
        // A shorter prefix must not swallow a longer one.
        assert_eq!(
            super::session_name_owner_pid("janitor-worker-4242-abcd1234", "janitor"),
            None
        );
        assert_eq!(
            super::session_name_owner_pid("janitor-workermore-4242-abcd1234", "janitor-worker"),
            None
        );
        // Names without an embedded pid, and malformed ones, are not ours.
        assert_eq!(
            super::session_name_owner_pid("janitor-worker-abcd1234", "janitor-worker"),
            None
        );
        assert_eq!(
            super::session_name_owner_pid("janitor-worker-4242-abcd123", "janitor-worker"),
            None
        );
        assert_eq!(
            super::session_name_owner_pid("janitor-worker-0-abcd1234", "janitor-worker"),
            None
        );
        assert_eq!(
            super::session_name_owner_pid("janitor-worker--4242-abcd1234", "janitor-worker"),
            None
        );
    }

    #[test]
    fn test_generate_session_id() {
        let id = super::generate_session_id("foo");
        assert_eq!(
            super::session_name_owner_pid(&id, "foo"),
            Some(std::process::id() as libc::pid_t)
        );
    }

    #[test]
    fn test_generate_session_id_sanitizes_prefix() {
        let id = super::generate_session_id("janitor@worker");
        assert!(id.starts_with("janitorworker-"), "{}", id);
        assert_eq!(
            super::session_name_owner_pid(&id, "janitorworker"),
            Some(std::process::id() as libc::pid_t)
        );
    }

    /// A pid that has certainly exited: spawned, waited for, and reaped.
    fn dead_pid() -> libc::pid_t {
        let mut child = std::process::Command::new("/bin/true")
            .spawn()
            .expect("spawn /bin/true");
        let pid = child.id() as libc::pid_t;
        child.wait().expect("wait for /bin/true");
        pid
    }

    #[test]
    fn test_pid_is_alive() {
        assert!(super::pid_is_alive(std::process::id() as libc::pid_t));
        assert!(super::pid_is_alive(1));
        assert!(!super::pid_is_alive(dead_pid()));
    }

    #[test]
    fn test_stale_session_names_skips_live_pid() {
        let dead = dead_pid();
        let stdout = format!(
            "session:janitor-worker-{}-aaaaaaaa\nsession:janitor-worker-{}-bbbbbbbb\n",
            std::process::id(),
            dead
        );
        assert_eq!(
            super::stale_session_names(&stdout, "janitor-worker").unwrap(),
            vec![format!("janitor-worker-{}-bbbbbbbb", dead)]
        );
    }

    #[test]
    fn test_stale_session_names_does_not_swallow_longer_prefix() {
        let stdout = format!("session:janitor-worker-{}-aaaaaaaa\n", dead_pid());
        assert!(super::stale_session_names(&stdout, "janitor")
            .unwrap()
            .is_empty());
        assert_eq!(
            super::stale_session_names(&stdout, "janitor-worker")
                .unwrap()
                .len(),
            1
        );
    }

    #[test]
    fn test_stale_session_names_rejects_empty_prefix() {
        let stdout = "session:-4242-aaaaaaaa\n";
        for prefix in ["", "@@@", "!"] {
            assert!(matches!(
                super::stale_session_names(stdout, prefix),
                Err(super::Error::InvalidSessionPrefix(_))
            ));
        }
    }

    #[test]
    fn test_stale_session_names_sanitizes_prefix() {
        let name = format!("janitorworker-{}-aaaaaaaa", dead_pid());
        let stdout = format!("session:{}\n", name);
        assert_eq!(
            super::stale_session_names(&stdout, "janitor@worker").unwrap(),
            vec![name]
        );
    }

    #[test]
    fn test_stale_session_names_trims_before_stripping_prefix() {
        let name = format!("janitor-worker-{}-aaaaaaaa", dead_pid());
        let stdout = format!("  session:{}  \n\n", name);
        assert_eq!(
            super::stale_session_names(&stdout, "janitor-worker").unwrap(),
            vec![name]
        );
    }

    #[test]
    fn test_stale_session_names_accepts_unqualified_names() {
        let name = format!("janitor-worker-{}-aaaaaaaa", dead_pid());
        let stdout = format!("{}\n", name);
        assert_eq!(
            super::stale_session_names(&stdout, "janitor-worker").unwrap(),
            vec![name]
        );
    }

    #[test]
    fn test_new_returns_missing_binary_when_schroot_absent() {
        // Only meaningful when the schroot binary isn't installed; when it
        // is, `new` will still hit the schroot-invocation path and return
        // some other error (which is fine).
        if std::process::Command::new("schroot")
            .arg("--version")
            .stdout(std::process::Stdio::null())
            .stderr(std::process::Stdio::null())
            .status()
            .is_ok()
        {
            return;
        }
        let err = super::SchrootSession::new("does-not-exist", None)
            .err()
            .expect("expected error when schroot binary is missing");
        assert!(
            matches!(err, super::Error::MissingBinary { ref command, .. } if command == "schroot"),
            "expected MissingBinary {{ command: \"schroot\", .. }}, got {:?}",
            err
        );
    }
}
