use crate::session::{run_with_tee, Error as SessionError, Session};
use buildlog_consultant::problems::common::MissingCommand;

fn default_check_success(status: std::process::ExitStatus, _lines: Vec<&str>) -> bool {
    status.success()
}

#[derive(Debug)]
/// Error type for analyzed command execution errors.
///
/// This enum represents different kinds of errors that can occur when running
/// and analyzing commands, with details about the specific error.
pub enum AnalyzedError {
    /// Error indicating a command was not found.
    MissingCommandError {
        /// The name of the command that was not found.
        command: String,
    },
    /// Error from an IO operation.
    IoError(std::io::Error),
    /// Detailed error with information from the buildlog consultant.
    Detailed {
        /// The return code of the failed command.
        retcode: i32,
        /// The specific build problem identified.
        error: Box<dyn buildlog_consultant::Problem>,
    },
    /// Error that could not be specifically identified.
    Unidentified {
        /// The return code of the failed command.
        retcode: i32,
        /// The output lines from the command.
        lines: Vec<String>,
        /// Optional secondary information about the error.
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
}

impl From<std::io::Error> for AnalyzedError {
    fn from(e: std::io::Error) -> Self {
        #[cfg(unix)]
        match e.raw_os_error() {
            Some(libc::ENOSPC) => {
                return AnalyzedError::Detailed {
                    retcode: 1,
                    error: Box::new(buildlog_consultant::problems::common::NoSpaceOnDevice),
                };
            }
            Some(libc::EMFILE) => {
                return AnalyzedError::Detailed {
                    retcode: 1,
                    error: Box::new(buildlog_consultant::problems::common::TooManyOpenFiles),
                }
            }
            _ => {}
        }
        AnalyzedError::IoError(e)
    }
}

impl std::fmt::Display for AnalyzedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            AnalyzedError::MissingCommandError { command } => {
                write!(f, "Command not found: {}", command)
            }
            AnalyzedError::IoError(e) => write!(f, "IO error: {}", e),
            AnalyzedError::Detailed { retcode, error } => {
                write!(f, "Command failed with code {}", retcode)?;
                write!(f, "\n{}", error)
            }
            AnalyzedError::Unidentified {
                retcode,
                lines,
                secondary,
            } => {
                write!(f, "Command failed with code {}", retcode)?;
                if let Some(secondary) = secondary {
                    write!(f, "\n{}", secondary)
                } else {
                    write!(f, "\n{}", lines.join("\n"))
                }
            }
        }
    }
}

impl std::error::Error for AnalyzedError {}

/// Run a command and analyze the output for common build errors.
///
/// # Arguments
/// * `session`: Session to run the command in
/// * `args`: Arguments to the command
/// * `check_success`: Function to determine if the command was successful
/// * `quiet`: Whether to log the command being run
/// * `cwd`: Current working directory for the command
/// * `user`: User to run the command as
/// * `env`: Environment variables to set for the command
/// * `stdin`: Stdin for the command
pub fn run_detecting_problems(
    session: &dyn Session,
    args: Vec<&str>,
    check_success: Option<&dyn Fn(std::process::ExitStatus, Vec<&str>) -> bool>,
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<&std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
) -> Result<Vec<String>, AnalyzedError> {
    let check_success = check_success.unwrap_or(&default_check_success);

    let (retcode, contents) =
        match run_with_tee(session, args.clone(), cwd, user, env, stdin, quiet) {
            Ok((retcode, contents)) => (retcode, contents),
            Err(SessionError::SetupFailure(..)) => unreachable!(),
            Err(SessionError::ImageError(..)) => unreachable!(),
            Err(SessionError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let command = args[0].to_string();
                return Err(AnalyzedError::Detailed {
                    retcode: 127,
                    error: Box::new(MissingCommand(command))
                        as Box<dyn buildlog_consultant::Problem>,
                });
            }
            Err(SessionError::IoError(e)) => {
                return Err(AnalyzedError::IoError(e));
            }
            Err(SessionError::CalledProcessError(retcode)) => (retcode, vec![]),
        };

    log::debug!(
        "Command returned code {}, with {} lines of output.",
        retcode.code().unwrap_or(1),
        contents.len()
    );

    if check_success(retcode, contents.iter().map(|s| s.as_str()).collect()) {
        return Ok(contents);
    }
    let (r#match, error) = buildlog_consultant::common::find_build_failure_description(
        contents.iter().map(|x| x.as_str()).collect(),
    );
    if let Some(error) = error {
        log::debug!("Identified error: {}", error);
        Err(AnalyzedError::Detailed {
            retcode: retcode.code().unwrap_or(1),
            error,
        })
    } else {
        if let Some(r#match) = r#match.as_ref() {
            log::warn!("Build failed with unidentified error:");
            log::warn!("{}", r#match.line().trim_end_matches('\n'));
        } else {
            log::warn!("Build failed without error being identified.");
        }
        Err(AnalyzedError::Unidentified {
            retcode: retcode.code().unwrap_or(1),
            lines: contents,
            secondary: r#match,
        })
    }
}
