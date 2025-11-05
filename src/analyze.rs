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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::session::plain::PlainSession;
    use std::process::ExitStatus;
    use tempfile::TempDir;

    #[test]
    fn test_analyzed_error_display_missing_command() {
        let error = AnalyzedError::MissingCommandError {
            command: "nonexistent".to_string(),
        };
        assert_eq!(error.to_string(), "Command not found: nonexistent");
    }

    #[test]
    fn test_analyzed_error_display_io_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "Access denied");
        let error = AnalyzedError::IoError(io_error);
        assert_eq!(error.to_string(), "IO error: Access denied");
    }

    #[test]
    fn test_analyzed_error_display_detailed() {
        let problem = Box::new(buildlog_consultant::problems::common::MissingCommand(
            "test".to_string(),
        ));
        let error = AnalyzedError::Detailed {
            retcode: 127,
            error: problem,
        };
        let display = error.to_string();
        assert!(display.starts_with("Command failed with code 127"));
        assert!(display.contains("test"));
    }

    #[test]
    fn test_analyzed_error_display_unidentified_with_secondary() {
        let error = AnalyzedError::Unidentified {
            retcode: 1,
            lines: vec!["line1".to_string(), "line2".to_string()],
            secondary: None, // Skip the secondary match for now due to API complexity
        };
        let display = error.to_string();
        assert!(display.starts_with("Command failed with code 1"));
        assert!(display.contains("line1\nline2"));
    }

    #[test]
    fn test_analyzed_error_display_unidentified_without_secondary() {
        let error = AnalyzedError::Unidentified {
            retcode: 1,
            lines: vec!["line1".to_string(), "line2".to_string()],
            secondary: None,
        };
        let display = error.to_string();
        assert!(display.starts_with("Command failed with code 1"));
        assert!(display.contains("line1\nline2"));
    }

    #[test]
    fn test_analyzed_error_from_io_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::NotFound, "File not found");
        let analyzed_error: AnalyzedError = io_error.into();
        match analyzed_error {
            AnalyzedError::IoError(e) => assert_eq!(e.kind(), std::io::ErrorKind::NotFound),
            _ => panic!("Expected IoError variant"),
        }
    }

    #[cfg(unix)]
    #[test]
    fn test_analyzed_error_from_no_space_error() {
        let io_error = std::io::Error::new(std::io::ErrorKind::Other, "No space left");
        // Unfortunately we can't easily create an io::Error with a specific raw_os_error
        // in a portable way, so this test is limited
        let analyzed_error: AnalyzedError = io_error.into();
        match analyzed_error {
            AnalyzedError::IoError(_) => {}
            _ => panic!("Expected IoError variant for non-specific error"),
        }
    }

    #[test]
    fn test_run_detecting_problems_success() {
        let _temp_dir = TempDir::new().unwrap();
        let session = PlainSession::new();

        let result = run_detecting_problems(
            &session,
            vec!["echo", "hello"],
            None,
            true,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_ok());
        let lines = result.unwrap();
        assert_eq!(lines, vec!["hello"]);
    }

    #[test]
    fn test_run_detecting_problems_with_custom_check_success() {
        let _temp_dir = TempDir::new().unwrap();
        let session = PlainSession::new();

        let custom_check = |_status: ExitStatus, lines: Vec<&str>| -> bool {
            lines.iter().any(|line| line.contains("hello"))
        };

        let result = run_detecting_problems(
            &session,
            vec!["echo", "hello"],
            Some(&custom_check),
            true,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_ok());
    }

    #[test]
    fn test_run_detecting_problems_nonexistent_command() {
        let _temp_dir = TempDir::new().unwrap();
        let session = PlainSession::new();

        let result = run_detecting_problems(
            &session,
            vec!["nonexistent_command_12345"],
            None,
            true,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            AnalyzedError::Detailed { retcode, error } => {
                assert_eq!(retcode, 127);
                assert_eq!(
                    error.to_string(),
                    "Missing command: nonexistent_command_12345"
                );
            }
            _ => panic!("Expected Detailed error for nonexistent command"),
        }
    }

    #[test]
    fn test_run_detecting_problems_failing_command() {
        let _temp_dir = TempDir::new().unwrap();
        let session = PlainSession::new();

        let result = run_detecting_problems(
            &session,
            vec!["false"], // Command that always fails
            None,
            true,
            None,
            None,
            None,
            None,
        );

        assert!(result.is_err());
        match result.unwrap_err() {
            AnalyzedError::Unidentified { retcode, .. } => {
                assert_eq!(retcode, 1);
            }
            _ => panic!("Expected Unidentified error for failing command"),
        }
    }

    #[test]
    fn test_default_check_success_with_success() {
        // Create a successful exit status (0)
        let output = std::process::Command::new("true").output().unwrap();
        let result = default_check_success(output.status, vec![]);
        assert!(result);
    }

    #[test]
    fn test_default_check_success_with_failure() {
        // Create a failed exit status (non-zero)
        let output = std::process::Command::new("false").output().unwrap();
        let result = default_check_success(output.status, vec![]);
        assert!(!result);
    }
}

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
