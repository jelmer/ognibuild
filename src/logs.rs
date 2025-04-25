use log::debug;
use std::fs;
use std::fs::File;
use std::io::{self, Write};
use std::os::unix::io::{AsRawFd, RawFd};
use std::path::{Path, PathBuf};
use std::process::Command;

struct RedirectOutput {
    old_stdout: RawFd,
    old_stderr: RawFd,
}

impl RedirectOutput {
    fn new(to_file: &File) -> io::Result<Self> {
        let stdout = io::stdout();
        let stderr = io::stderr();

        stdout.lock().flush()?;
        stderr.lock().flush()?;

        let old_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
        let old_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };

        if old_stdout == -1 || old_stderr == -1 {
            return Err(io::Error::last_os_error());
        }

        unsafe {
            libc::dup2(to_file.as_raw_fd(), libc::STDOUT_FILENO);
            libc::dup2(to_file.as_raw_fd(), libc::STDERR_FILENO);
        }

        Ok(RedirectOutput {
            old_stdout,
            old_stderr,
        })
    }
}

impl Drop for RedirectOutput {
    fn drop(&mut self) {
        let stdout = io::stdout();
        let stderr = io::stderr();

        stdout.lock().flush().unwrap();
        stderr.lock().flush().unwrap();

        unsafe {
            libc::dup2(self.old_stdout, libc::STDOUT_FILENO);
            libc::dup2(self.old_stderr, libc::STDERR_FILENO);
            libc::close(self.old_stdout);
            libc::close(self.old_stderr);
        }
    }
}

struct CopyOutput {
    old_stdout: RawFd,
    old_stderr: RawFd,
    new_fd: Option<RawFd>,
}

impl CopyOutput {
    fn new(output_log: &std::path::Path, tee: bool) -> io::Result<Self> {
        let old_stdout = unsafe { libc::dup(libc::STDOUT_FILENO) };
        let old_stderr = unsafe { libc::dup(libc::STDERR_FILENO) };

        let new_fd = if tee {
            let process = Command::new("tee")
                .arg(output_log)
                .stdin(std::process::Stdio::piped())
                .spawn()?;
            process.stdin.unwrap().as_raw_fd()
        } else {
            File::create(output_log)?.as_raw_fd()
        };

        unsafe {
            libc::dup2(new_fd, libc::STDOUT_FILENO);
            libc::dup2(new_fd, libc::STDERR_FILENO);
        }

        Ok(CopyOutput {
            old_stdout,
            old_stderr,
            new_fd: Some(new_fd),
        })
    }
}

impl Drop for CopyOutput {
    fn drop(&mut self) {
        if let Some(fd) = self.new_fd.take() {
            unsafe {
                libc::fsync(fd);
                libc::close(fd);
            }
        }

        unsafe {
            libc::dup2(self.old_stdout, libc::STDOUT_FILENO);
            libc::dup2(self.old_stderr, libc::STDERR_FILENO);
            libc::close(self.old_stdout);
            libc::close(self.old_stderr);
        }
    }
}

/// Rotate a log file, moving it to a new file with a timestamp.
///
/// # Arguments
/// * `source_path` - Path to the log file to rotate
///
/// # Returns
/// * `Ok(())` - If the log file was rotated successfully
/// * `Err(Error)` - If rotating the log file failed
pub fn rotate_logfile(source_path: &std::path::Path) -> std::io::Result<()> {
    if source_path.exists() {
        let directory_path = source_path.parent().unwrap_or_else(|| Path::new(""));
        let name = source_path.file_name().unwrap().to_str().unwrap();

        let mut i = 1;
        while directory_path.join(format!("{}.{}", name, i)).exists() {
            i += 1;
        }

        let target_path: PathBuf = directory_path.join(format!("{}.{}", name, i));
        fs::rename(source_path, &target_path)?;

        debug!("Storing previous build log at {}", target_path.display());
    }
    Ok(())
}

/// Mode for logging.
pub enum LogMode {
    /// Copy output to the log file.
    Copy,
    /// Redirect output to the log file.
    Redirect,
}

/// Trait for managing log files for build operations.
pub trait LogManager {
    /// Start logging to the log file.
    ///
    /// # Returns
    /// * `Ok(())` - If logging was started successfully
    /// * `Err(Error)` - If starting logging failed
    fn start(&mut self) -> std::io::Result<()>;

    /// Stop logging to the log file.
    fn stop(&mut self) {}
}

/// Run a function capturing its output to a log file.
pub fn wrap<R>(logs: &mut dyn LogManager, f: impl FnOnce() -> R) -> R {
    logs.start().unwrap();
    let result = f();
    std::io::stdout().flush().unwrap();
    std::io::stderr().flush().unwrap();
    logs.stop();
    result
}

/// Log manager that logs to a file in a directory.
pub struct DirectoryLogManager {
    path: PathBuf,
    mode: LogMode,
    copy_output: Option<CopyOutput>,
    redirect_output: Option<RedirectOutput>,
}

impl DirectoryLogManager {
    /// Create a new DirectoryLogManager.
    ///
    /// # Arguments
    /// * `path` - Path to the log file
    /// * `mode` - Mode for logging
    ///
    /// # Returns
    /// A new DirectoryLogManager instance
    pub fn new(path: PathBuf, mode: LogMode) -> Self {
        Self {
            path,
            mode,
            copy_output: None,
            redirect_output: None,
        }
    }
}

impl LogManager for DirectoryLogManager {
    fn start(&mut self) -> std::io::Result<()> {
        rotate_logfile(&self.path)?;
        match self.mode {
            LogMode::Copy => {
                self.copy_output = Some(CopyOutput::new(&self.path, true)?);
            }
            LogMode::Redirect => {
                self.redirect_output = Some(RedirectOutput::new(&File::create(&self.path)?)?);
            }
        }
        Ok(())
    }

    fn stop(&mut self) {
        self.copy_output = None;
        self.redirect_output = None;
    }
}

/// Log manager that does nothing.
pub struct NoLogManager;

impl NoLogManager {
    /// Create a new NoLogManager.
    ///
    /// # Returns
    /// A new NoLogManager instance
    pub fn new() -> Self {
        Self {}
    }
}

impl Default for NoLogManager {
    fn default() -> Self {
        Self::new()
    }
}

impl LogManager for NoLogManager {
    fn start(&mut self) -> std::io::Result<()> {
        Ok(())
    }
}
