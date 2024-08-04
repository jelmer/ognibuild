use crate::session::Session;

#[derive(Debug)]
pub enum Error {
    Unidentified {
        retcode: i32,
        args: Vec<String>,
        lines: Vec<String>,
    },
    Detailed {
        retcode: i32,
        args: Vec<String>,
        error: String,
    },
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unidentified { retcode, args, lines } => {
                write!(f, "apt failed with retcode {}: {:?}\n{}", retcode, args, lines.join("\n"))
            }
            Error::Detailed { retcode, args, error } => {
                write!(f, "apt failed with retcode {}: {:?}\n{}", retcode, args, error)
            }
        }
    }
}

impl std::error::Error for Error {}
