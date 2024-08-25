use crate::session::{get_user, Session};

pub enum Error {
    Unidentified {
        retcode: i32,
        args: Vec<String>,
        lines: Vec<String>,
        secondary: Option<Box<dyn buildlog_consultant::Match>>,
    },
    Detailed {
        retcode: i32,
        args: Vec<String>,
        error: Option<Box<dyn buildlog_consultant::Problem>>,
    },
    Session(crate::session::Error),
}

impl std::fmt::Debug for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unidentified {
                retcode,
                args,
                lines,
                secondary: _,
            } => {
                write!(
                    f,
                    "Unidentified error: apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    lines.join("\n")
                )
            }
            Error::Detailed {
                retcode,
                args,
                error,
            } => {
                write!(
                    f,
                    "Detailed error: apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    error.as_ref().map_or("".to_string(), |e| e.to_string())
                )
            }
            Error::Session(error) => write!(f, "{:?}", error),
        }
    }
}

impl From<crate::session::Error> for Error {
    fn from(error: crate::session::Error) -> Self {
        Error::Session(error)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::Unidentified {
                retcode,
                args,
                lines,
                secondary: _,
            } => {
                write!(
                    f,
                    "apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    lines.join("\n")
                )
            }
            Error::Detailed {
                retcode,
                args,
                error,
            } => {
                write!(
                    f,
                    "apt failed with retcode {}: {:?}\n{}",
                    retcode,
                    args,
                    error.as_ref().map_or("".to_string(), |e| e.to_string())
                )
            }
            Error::Session(error) => write!(f, "{}", error),
        }
    }
}

impl std::error::Error for Error {}

pub struct AptManager<'a> {
    session: &'a dyn Session,
    prefix: Vec<String>,
    searchers: Option<Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>>>,
}

impl<'a> AptManager<'a> {
    pub fn new(session: &'a dyn Session, prefix: Option<Vec<String>>) -> Self {
        Self {
            session,
            prefix: prefix.unwrap_or_default(),
            searchers: None,
        }
    }

    pub fn searchers(&'a mut self) -> &Vec<Box<dyn crate::debian::file_search::FileSearcher<'a> + 'a>> {
        if self.searchers.is_none() {
            self.searchers = Some(vec![
                crate::debian::file_search::get_apt_contents_file_searcher(self.session).unwrap(),
                Box::new(crate::debian::file_search::GENERATED_FILE_SEARCHER.clone()),
            ]);
        }
        self.searchers.as_ref().unwrap()
    }

    pub fn from_session(session: &'a dyn Session) -> Self {
        let prefix = if get_user(session).as_str() != "root" {
            vec!["sudo".to_string()]
        } else {
            vec![]
        };
        return Self::new(session, Some(prefix));
    }

    fn run_apt(&self, args: Vec<&str>) -> Result<(), Error> {
        run_apt(
            self.session,
            args,
            self.prefix.iter().map(|s| s.as_str()).collect(),
        )
    }

    pub fn satisfy(&self, deps: Vec<&str>) -> Result<(), Error> {
        let mut args = vec!["satisfy"];
        args.extend(deps);
        self.run_apt(args)
    }

    pub fn satisfy_command<'b>(&'b self, deps: Vec<&'b str>) -> Vec<&'b str> {
        let mut args = self
            .prefix
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<&str>>();
        args.push("apt");
        args.push("satisfy");
        args.extend(deps);
        args
    }
}

pub fn run_apt(session: &dyn Session, args: Vec<&str>, prefix: Vec<&str>) -> Result<(), Error> {
    let args = [prefix, vec!["apt", "-y"], args].concat();
    log::info!("apt: running {:?}", args);
    let (retcode, mut lines) = crate::session::run_with_tee(
        session,
        args.clone(),
        Some(std::path::Path::new("/")),
        Some("root"),
        None,
        None,
        None,
        None,
    )?;
    if retcode == 0 {
        return Ok(());
    }
    let (r#match, error) =
        buildlog_consultant::apt::find_apt_get_failure(lines.iter().map(|s| s.as_str()).collect());
    if let Some(error) = error {
        return Err(Error::Detailed {
            retcode,
            args: args.iter().map(|s| s.to_string()).collect(),
            error: Some(error),
        });
    }
    while lines.last().map_or(false, |line| line.trim().is_empty()) {
        lines.pop();
    }
    return Err(Error::Unidentified {
        retcode,
        args: args.iter().map(|s| s.to_string()).collect(),
        lines,
        secondary: r#match,
    });
}
