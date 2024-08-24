use crate::session::{Session, Error as SessionError, run_with_tee};
use buildlog_consultant::problems::common::MissingCommand;

fn default_check_success(retcode: i32, _lines: Vec<&str>) -> bool {
    retcode == 0
}

#[derive(Debug)]
pub enum AnalyzedError {
    MissingCommandError {
        command: String,
    },
    IoError(std::io::Error),
    Detailed {
        retcode: i32,
        error: Option<Box<dyn buildlog_consultant::Problem>>,
    },
    Unidentified {
        retcode: i32,
        lines: Vec<String>,
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
}

impl From<std::io::Error> for AnalyzedError {
    fn from(e: std::io::Error) -> Self {
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
            AnalyzedError::Detailed {
                retcode,
                error,
            } => {
                write!(f, "Command failed with code {}", retcode)?;
                if let Some(error) = error {
                    write!(f, "\n{}", error)
                } else {
                    Ok(())
                }
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
/// * `stdout`: Stdout for the command
/// * `stderr`: Stderr for the command
pub fn run_detecting_problems(
    session: &dyn Session,
    args: Vec<&str>,
    check_success: Option<&dyn Fn(i32, Vec<&str>) -> bool>,
    quiet: bool,
    cwd: Option<&std::path::Path>,
    user: Option<&str>,
    env: Option<std::collections::HashMap<String, String>>,
    stdin: Option<std::process::Stdio>,
    stdout: Option<std::process::Stdio>,
    stderr: Option<std::process::Stdio>,
) -> Result<Vec<String>, AnalyzedError> {
    if !quiet {
        log::info!("Running {:?}", args);
    }
    let check_success = check_success.unwrap_or(&default_check_success);

    let (retcode, contents) =
        match run_with_tee(session, args.clone(), cwd, user, env, stdin, stdout, stderr) {
            Ok((retcode, contents)) => (retcode, contents),
            Err(SessionError::SetupFailure(..)) => unreachable!(),
            Err(SessionError::IoError(e)) if e.kind() == std::io::ErrorKind::NotFound => {
                let command = args[0].to_string();
                return Err(AnalyzedError::Detailed {
                    retcode: 127,
                    error: Some(
                        Box::new(MissingCommand(command)) as Box<dyn buildlog_consultant::Problem>
                    ),
                });
            }
            Err(SessionError::IoError(e)) => {
                return Err(AnalyzedError::IoError(e));
            }
            Err(SessionError::CalledProcessError(retcode)) => (retcode, vec![]),
        };
    if check_success(retcode, contents.iter().map(|s| s.as_str()).collect()) {
        return Ok(contents);
    }
    let body = contents.join("");
    let lines = body.split('\n').collect::<Vec<_>>();
    let (r#match, error) =
        buildlog_consultant::common::find_build_failure_description(lines.clone());
    if let Some(error) = error {
        Err(AnalyzedError::Detailed {
            retcode,
            error: Some(error),
        })
    } else {
        if let Some(r#match) = r#match.as_ref() {
            log::warn!("Build failed with unidentified error:");
            log::warn!("{}", r#match.line().trim_end_matches('\n'));
        } else {
            log::warn!("Build failed and unable to find cause. Giving up.");
        }
        Err(AnalyzedError::Unidentified {
            retcode,
            lines: lines.into_iter().map(|s| s.to_string()).collect(),
            secondary: r#match,
        })
    }
}
