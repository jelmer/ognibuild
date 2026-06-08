//! File searching utilities for Debian packages.
//!
//! This module provides functionality for searching files in Debian
//! packages, including using apt-file and other package contents databases.

use crate::debian::apt::repository::{
    contents_urls_from_sources, load_direct_url, load_url_with_cache, read_contents_file, Error,
};
use crate::session::{Error as SessionError, Session};
use std::collections::HashMap;
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};

/// Trait for searching files in Debian packages.
///
/// Implementors of this trait provide methods for searching files
/// by exact path or regular expression.
pub trait FileSearcher<'b> {
    /// Search for files by exact path.
    ///
    /// # Arguments
    /// * `path` - Path to search for
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing the file
    fn search_files<'a>(
        &'a self,
        path: &'a Path,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a>;

    /// Search for files by regular expression.
    ///
    /// # Arguments
    /// * `path` - Regular expression pattern to match against file paths
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_regex<'a>(
        &'a self,
        path: &'a str,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a>;
}

lazy_static::lazy_static! {
    /// Path to the file that indicates the apt-file cache is empty.
    pub static ref CACHE_IS_EMPTY_PATH: &'static Path = Path::new("/usr/share/apt-file/is-cache-empty");
}

/// File searcher that uses apt-file to find files in Debian packages.
pub struct AptFileFileSearcher<'a> {
    /// Session for running commands
    session: &'a dyn Session,
}

impl<'a> AptFileFileSearcher<'a> {
    /// Check if the apt-file cache exists and is not empty.
    ///
    /// # Arguments
    /// * `session` - Session for running commands
    ///
    /// # Returns
    /// `true` if the cache exists and is not empty, `false` otherwise
    pub fn has_cache(session: &dyn Session) -> Result<bool, SessionError> {
        if !session.exists(&CACHE_IS_EMPTY_PATH) {
            return Ok(false);
        }
        match session
            .command(vec![&CACHE_IS_EMPTY_PATH.to_str().unwrap()])
            .check_call()
        {
            Ok(_) => Ok(true),
            Err(SessionError::CalledProcessError(status)) => {
                if status.code() == Some(1) {
                    Ok(true)
                } else {
                    Ok(false)
                }
            }
            Err(e) => Err(e),
        }
    }

    /// Create a new AptFileFileSearcher from a session.
    ///
    /// This ensures that apt-file is installed and the cache is updated.
    ///
    /// # Arguments
    /// * `session` - Session for running commands
    ///
    /// # Returns
    /// A new AptFileFileSearcher instance
    pub fn from_session(session: &dyn Session) -> AptFileFileSearcher<'_> {
        log::debug!("Using apt-file to search apt contents");
        if !session.exists(&CACHE_IS_EMPTY_PATH) {
            crate::debian::apt::AptManager::from_session(session)
                .satisfy(vec![crate::debian::apt::SatisfyEntry::Required(
                    "apt-file".to_string(),
                )])
                .unwrap();
        }
        if !Self::has_cache(session).unwrap() {
            session
                .command(vec!["apt-file", "update"])
                .user("root")
                .check_call()
                .unwrap();
        }
        AptFileFileSearcher { session }
    }

    /// Search for files in Debian packages.
    ///
    /// This is an internal implementation method used by the FileSearcher trait methods.
    ///
    /// # Arguments
    /// * `path` - Path or pattern to search for
    /// * `regex` - Whether to treat the path as a regular expression
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_ex(
        &self,
        path: &str,
        regex: bool,
        case_insensitive: bool,
    ) -> Result<impl Iterator<Item = String>, Error> {
        let mut args = vec!["apt-file", "search", "--stream-results"];
        if regex {
            args.push("-x");
        } else {
            args.push("-F");
        }
        if case_insensitive {
            args.push("-i");
        }
        args.push(path);
        let output = self
            .session
            .command(args)
            .stdout(std::process::Stdio::piped())
            .stderr(std::process::Stdio::piped())
            .output()
            .map_err(|e| {
                Error::AptFileAccessError(format!(
                    "Unable to search for files matching {}: {}",
                    path, e
                ))
            })?;
        match output.status.code() {
            Some(0) | Some(1) => {
                // At least one search result
                let output_str = std::str::from_utf8(&output.stdout).unwrap();
                let entries = output_str
                    .split('\n')
                    .filter_map(|line| {
                        if line.is_empty() {
                            return None;
                        }
                        let (pkg, _path) = line.split_once(": ").unwrap();
                        Some(pkg.to_string())
                    })
                    .collect::<Vec<String>>();
                log::debug!("Found entries {:?} for {}", entries, path);
                Ok(entries.into_iter())
            }
            Some(2) => {
                // Error
                Err(Error::AptFileAccessError(format!(
                    "Error searching for files matching {}: {}",
                    path,
                    std::str::from_utf8(&output.stderr).unwrap()
                )))
            }
            Some(3) => Err(Error::AptFileAccessError(
                "apt-file cache is empty".to_owned(),
            )),
            Some(4) => Err(Error::AptFileAccessError(
                "apt-file has no entries matching restrictions".to_owned(),
            )),
            _ => Err(Error::AptFileAccessError(
                "apt-file returned an unknown error".to_owned(),
            )),
        }
    }
}

impl<'b> FileSearcher<'b> for AptFileFileSearcher<'b> {
    /// Search for files by exact path.
    ///
    /// This implementation uses apt-file to search for packages
    /// containing the specified file path.
    ///
    /// # Arguments
    /// * `path` - Path to search for
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing the file
    fn search_files<'a>(
        &'a self,
        path: &'a Path,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        return Box::new(
            self.search_files_ex(path.to_str().unwrap(), false, case_insensitive)
                .unwrap(),
        );
    }

    /// Search for files by regular expression.
    ///
    /// This implementation uses apt-file with the -x flag to search for packages
    /// containing files matching the specified regex pattern.
    ///
    /// # Arguments
    /// * `path` - Regular expression pattern to match against file paths
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_regex<'a>(
        &'a self,
        path: &'a str,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        Box::new(self.search_files_ex(path, true, case_insensitive).unwrap())
    }
}

/// Set up apt-file in a session.
///
/// This function installs apt-file if needed and updates the apt-file cache.
///
/// # Arguments
/// * `session` - Session to set up
///
/// # Returns
/// Ok(()) if setup was successful, Error otherwise
pub fn setup_apt_file(session: &dyn Session) -> Result<(), Error> {
    // Update APT package lists first
    log::info!("Updating APT package lists...");
    session
        .command(vec!["apt-get", "update"])
        .user("root")
        .check_call()
        .map_err(|e| Error::AptFileAccessError(format!("Failed to run apt-get update: {}", e)))?;

    // Install apt-file if not already installed
    log::info!("Installing apt-file...");
    session
        .command(vec!["apt-get", "install", "-y", "apt-file"])
        .user("root")
        .check_call()
        .map_err(|e| Error::AptFileAccessError(format!("Failed to install apt-file: {}", e)))?;

    // Update apt-file cache
    log::info!("Updating apt-file cache...");
    session
        .command(vec!["apt-file", "update"])
        .user("root")
        .check_call()
        .map_err(|e| Error::AptFileAccessError(format!("Failed to run apt-file update: {}", e)))?;

    log::info!("apt-file setup complete");
    Ok(())
}

/// Get a file searcher that uses apt-file or remote contents.
///
/// This function returns the appropriate file searcher based on whether
/// apt-file cache is available. If apt-file cache is available, it returns
/// an AptFileFileSearcher; otherwise, it returns a RemoteContentsFileSearcher.
///
/// # Arguments
/// * `session` - Session for running commands
///
/// # Returns
/// A file searcher implementation
pub fn get_apt_contents_file_searcher<'a>(
    session: &'a dyn Session,
) -> Result<Box<dyn FileSearcher<'a> + 'a>, Error> {
    if AptFileFileSearcher::has_cache(session).unwrap() {
        Ok(Box::new(AptFileFileSearcher::from_session(session)) as Box<dyn FileSearcher<'a>>)
    } else {
        // Try to load remote contents, but with timeouts to prevent hanging
        RemoteContentsFileSearcher::from_session(session)
            .map(|searcher| Box::new(searcher) as Box<dyn FileSearcher<'a>>)
    }
}

/// File searcher that uses remote Contents files from Debian repositories.
///
/// This searcher downloads and parses Contents files from Debian repositories
/// to find packages containing specific files.
pub struct RemoteContentsFileSearcher {
    /// Database mapping file paths to package names
    db: HashMap<String, Vec<u8>>,
}

impl RemoteContentsFileSearcher {
    /// Create a new RemoteContentsFileSearcher from a session.
    ///
    /// This loads contents information from the APT sources configured in
    /// the session.
    ///
    /// # Arguments
    /// * `session` - Session for running commands
    ///
    /// # Returns
    /// A new RemoteContentsFileSearcher instance
    pub fn from_session(session: &dyn Session) -> Result<RemoteContentsFileSearcher, Error> {
        log::debug!("Loading apt contents information");
        let mut ret = RemoteContentsFileSearcher { db: HashMap::new() };
        ret.load_from_session(session)?;
        Ok(ret)
    }

    /// Load contents information from local APT sources.
    ///
    /// # Returns
    /// Ok(()) if successful, Error otherwise
    pub fn load_local(&mut self) -> Result<(), Error> {
        let repositories = apt_sources::Repositories::default();
        let arch = crate::debian::build::get_build_architecture();
        let cache_dirs = vec![Path::new("/var/lib/apt/lists")];
        let load_url = |url: &url::Url| load_url_with_cache(url, cache_dirs.as_slice());
        let urls = contents_urls_from_sources(
            &repositories,
            &arch,
            load_url,
            crate::debian::apt::verify::trusted_certs_host,
        );
        self.load_urls(urls, load_url, false)
    }

    /// Load contents information from APT sources configured in a session.
    ///
    /// # Arguments
    /// * `session` - Session for running commands
    ///
    /// # Returns
    /// Ok(()) if successful, Error otherwise
    pub fn load_from_session(&mut self, session: &dyn Session) -> Result<(), Error> {
        let (repositories, _errors) = apt_sources::Repositories::load_from_directory(
            &session.external_path(Path::new("/etc/apt")),
        );
        let arch = crate::debian::build::get_build_architecture();
        let cache_dirs = [session.external_path(Path::new("/var/lib/apt/lists"))];
        let load_url = |url: &url::Url| {
            load_url_with_cache(
                url,
                cache_dirs
                    .iter()
                    .map(|p| p.as_ref())
                    .collect::<Vec<&Path>>()
                    .as_slice(),
            )
        };
        // Keyrings referenced by Signed-By (and APT's default trusted keyrings)
        // live inside the session, so resolve their paths through the session's
        // filesystem before reading them.
        let resolve_certs = |signature: Option<&apt_sources::signature::Signature>| {
            crate::debian::apt::verify::trusted_certs(
                signature,
                |path| std::fs::read(session.external_path(path)),
                |dir| {
                    std::fs::read_dir(session.external_path(dir))?
                        .map(|entry| entry.map(|e| e.path()))
                        .collect()
                },
            )
        };
        let urls = contents_urls_from_sources(&repositories, &arch, load_url, resolve_certs);
        self.load_urls(urls, load_url, false)
    }

    /// Load contents information from multiple URLs.
    ///
    /// # Arguments
    /// * `urls` - Iterator of URLs to load
    /// * `load_url` - Function to load a URL and get a reader
    ///
    /// # Returns
    /// Ok(()) if successful, Error otherwise
    fn load_urls(
        &mut self,
        urls: impl Iterator<Item = url::Url>,
        load_url: impl Fn(&url::Url) -> Result<Box<dyn Read>, Error>,
        fail_on_error: bool,
    ) -> Result<(), Error> {
        let urls: Vec<url::Url> = urls.collect();
        let num_urls = urls.len();

        if num_urls == 0 {
            return Ok(());
        }

        log::info!(
            "Loading {} APT Contents files (this may take several minutes)...",
            num_urls
        );

        let mut success_count = 0;
        let mut contents = Vec::new();

        // Try to load each URL
        for (idx, url) in urls.iter().enumerate() {
            log::info!("Loading Contents file {}/{}: {}", idx + 1, num_urls, url);
            match load_url(&url) {
                Ok(reader) => {
                    // Read the entire content into memory
                    let mut content = Vec::new();
                    let mut reader = reader;
                    match std::io::Read::read_to_end(&mut reader, &mut content) {
                        Ok(size) => {
                            log::info!("Successfully loaded {} bytes from {}", size, url);
                            contents.push((url.clone(), content));
                            success_count += 1;
                        }
                        Err(e) => {
                            if fail_on_error {
                                return Err(Error::AptFileAccessError(format!(
                                    "Failed to read Contents from {}: {}",
                                    url, e
                                )));
                            } else {
                                log::warn!("Failed to read Contents from {}: {}", url, e);
                            }
                        }
                    }
                }
                Err(e) => {
                    if fail_on_error {
                        return Err(e);
                    } else {
                        log::warn!("Failed to load Contents from {}: {}", url, e);
                    }
                }
            }
        }

        log::info!(
            "Successfully loaded {}/{} Contents files",
            success_count,
            num_urls
        );

        if success_count == 0 {
            return Err(Error::AptFileAccessError(
                "Failed to download any APT Contents files".to_string(),
            ));
        }

        // Process the successfully loaded files
        for (url, content) in contents {
            let reader = Box::new(std::io::Cursor::new(content));
            self.load_file(reader, url);
        }

        Ok(())
    }

    /// Search for files in Debian packages using a matcher function.
    ///
    /// This is an internal implementation method used by the FileSearcher trait methods.
    ///
    /// # Arguments
    /// * `matches` - Function that returns true for paths that match the search criteria
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    pub fn search_files_ex<'a>(
        &'a self,
        mut matches: impl FnMut(&Path) -> bool + 'a,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        Box::new(
            self.db
                .iter()
                .filter(move |(p, _)| matches(Path::new(p)))
                .map(|(_, rest)| {
                    std::str::from_utf8(rest.split(|c| *c == b'/').last().unwrap())
                        .unwrap()
                        .to_string()
                }),
        )
    }

    /// Load contents information from a file.
    ///
    /// # Arguments
    /// * `f` - Reader for the contents file
    /// * `url` - URL of the contents file (for logging)
    fn load_file(&mut self, f: impl Read, url: url::Url) {
        let start_time = std::time::Instant::now();
        for (path, rest) in read_contents_file(f) {
            self.db.insert(path, rest.into());
        }
        log::debug!("Read {} in {:?}", url, start_time.elapsed());
    }
}

impl FileSearcher<'_> for RemoteContentsFileSearcher {
    /// Search for files by exact path.
    ///
    /// This implementation uses the remote Contents database to find packages
    /// containing the specified file path.
    ///
    /// # Arguments
    /// * `path` - Path to search for
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing the file
    fn search_files<'a>(
        &'a self,
        path: &'a Path,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let path = if case_insensitive {
            PathBuf::from(path.to_str().unwrap().to_lowercase())
        } else {
            path.to_owned()
        };
        return Box::new(self.search_files_ex(move |p| {
            if case_insensitive {
                p.to_str().unwrap().to_lowercase() == path.to_str().unwrap()
            } else {
                p == path
            }
        }));
    }

    /// Search for files by regular expression.
    ///
    /// This implementation uses the remote Contents database to find packages
    /// containing files matching the specified regex pattern.
    ///
    /// # Arguments
    /// * `path` - Regular expression pattern to match against file paths
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_regex<'a>(
        &'a self,
        path: &'a str,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let re = regex::RegexBuilder::new(path)
            .case_insensitive(case_insensitive)
            .build()
            .unwrap();

        return Box::new(self.search_files_ex(move |p| {
            if case_insensitive {
                re.is_match(&p.to_str().unwrap().to_lowercase())
            } else {
                re.is_match(p.to_str().unwrap())
            }
        }));
    }
}

#[derive(Debug, Clone)]
/// File searcher that uses a pre-generated list of file paths and package names.
///
/// This searcher is useful for static file path to package mappings that
/// are known in advance.
pub struct GeneratedFileSearcher {
    /// Database of file path and package name pairs
    db: Vec<(PathBuf, String)>,
}

impl GeneratedFileSearcher {
    /// Create a new GeneratedFileSearcher.
    pub fn new(db: Vec<(PathBuf, String)>) -> GeneratedFileSearcher {
        Self { db }
    }

    /// Create an empty GeneratedFileSearcher.
    pub fn empty() -> GeneratedFileSearcher {
        Self::new(vec![])
    }

    /// Create a new GeneratedFileSearcher from a file.
    ///
    /// # Arguments
    /// * `path` - The path to the file to load.
    pub fn from_path(path: &Path) -> GeneratedFileSearcher {
        let mut ret = Self::new(vec![]);
        ret.load_from_path(path);
        ret
    }

    /// Load the contents of a file into the database.
    ///
    /// # Arguments
    /// * `path` - The path to the file to load.
    pub fn load_from_path(&mut self, path: &Path) {
        let f = File::open(path).unwrap();
        let f = BufReader::new(f);
        for line in f.lines() {
            let line = line.unwrap();
            let (path, pkg) = line.split_once(' ').unwrap();
            self.db.push((path.into(), pkg.to_owned()));
        }
    }

    fn search_files_ex<'a>(
        &'a self,
        mut matches: impl FnMut(&Path) -> bool + 'a,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let x = self
            .db
            .iter()
            .filter(move |(p, _)| matches(p))
            .map(|(_, pkg)| pkg.to_string());
        Box::new(x)
    }
}

impl FileSearcher<'_> for GeneratedFileSearcher {
    /// Search for files by exact path.
    ///
    /// This implementation uses the pre-generated database to find packages
    /// containing the specified file path.
    ///
    /// # Arguments
    /// * `path` - Path to search for
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing the file
    fn search_files<'a>(
        &'a self,
        path: &'a Path,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let path = if case_insensitive {
            PathBuf::from(path.to_str().unwrap().to_lowercase())
        } else {
            path.to_owned()
        };
        self.search_files_ex(move |p: &Path| {
            if case_insensitive {
                PathBuf::from(p.to_str().unwrap().to_lowercase()) == path
            } else {
                p == path
            }
        })
    }

    /// Search for files by regular expression.
    ///
    /// This implementation uses the pre-generated database to find packages
    /// containing files matching the specified regex pattern.
    ///
    /// # Arguments
    /// * `path` - Regular expression pattern to match against file paths
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_regex<'a>(
        &'a self,
        path: &'a str,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let re = regex::RegexBuilder::new(path)
            .case_insensitive(case_insensitive)
            .build()
            .unwrap();
        return self.search_files_ex(move |p| re.is_match(p.to_str().unwrap()));
    }
}

// TODO(jelmer): read from a file
lazy_static::lazy_static! {
    /// Pre-generated static file searcher with common Debian package files.
    ///
    /// This provides a mapping of common file paths to their providing packages.
    pub static ref GENERATED_FILE_SEARCHER: GeneratedFileSearcher = GeneratedFileSearcher::new(vec![
        (PathBuf::from("/etc/locale.gen"), "locales".to_string()),
        // Alternative
        (PathBuf::from("/usr/bin/rst2html"), "python3-docutils".to_string()),
        // aclocal is a symlink to aclocal-1.XY
        (PathBuf::from("/usr/bin/aclocal"), "automake".to_string()),
        (PathBuf::from("/usr/bin/automake"), "automake".to_string()),
        // maven lives in /usr/share
        (PathBuf::from("/usr/bin/mvn"), "maven".to_string()),
    ]);
}

/// Get a list of packages that provide the given paths.
///
/// # Arguments
/// * `paths` - A list of paths to search for.
/// * `searchers` - A list of searchers to use.
/// * `regex` - Whether the paths are regular expressions.
/// * `case_insensitive` - Whether the search should be case-insensitive.
///
/// # Returns
/// A list of packages that provide the given paths.
/// Get packages that contain the specified paths.
///
/// # Arguments
/// * `paths` - Paths to search for
/// * `searchers` - File searchers to use
/// * `regex` - Whether to treat paths as regular expressions
/// * `case_insensitive` - Whether to ignore case when matching
///
/// # Returns
/// List of package names that contain the specified paths
pub fn get_packages_for_paths(
    paths: Vec<&str>,
    searchers: &[&dyn FileSearcher],
    regex: bool,
    case_insensitive: bool,
) -> Vec<String> {
    let mut candidates = vec![];
    // TODO(jelmer): Combine these, perhaps by creating one gigantic regex?
    for path in paths {
        for searcher in searchers {
            for pkg in if regex {
                searcher.search_files_regex(path, case_insensitive)
            } else {
                searcher.search_files(Path::new(path), case_insensitive)
            } {
                if !candidates.contains(&pkg) {
                    candidates.push(pkg);
                }
            }
        }
    }
    candidates
}

/// File searcher that uses an in-memory map of file paths to package names.
///
/// This searcher is more efficient for small datasets that can fit entirely
/// in memory.
pub struct MemoryAptSearcher(std::collections::HashMap<PathBuf, String>);

impl MemoryAptSearcher {
    /// Create a new MemoryAptSearcher with the given database.
    ///
    /// # Arguments
    /// * `db` - Map of file paths to package names
    ///
    /// # Returns
    /// A new MemoryAptSearcher instance
    pub fn new(db: std::collections::HashMap<PathBuf, String>) -> MemoryAptSearcher {
        MemoryAptSearcher(db)
    }
}

impl FileSearcher<'_> for MemoryAptSearcher {
    /// Search for files by exact path.
    ///
    /// This implementation uses the in-memory database to find packages
    /// containing the specified file path.
    ///
    /// # Arguments
    /// * `path` - Path to search for
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing the file
    fn search_files<'a>(
        &'a self,
        path: &'a Path,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        if case_insensitive {
            Box::new(
                self.0
                    .iter()
                    .filter(move |(p, _)| {
                        p.to_str().unwrap().to_lowercase() == path.to_str().unwrap()
                    })
                    .map(|(_, pkg)| pkg.to_string()),
            )
        } else {
            let hit = self.0.get(path);
            if let Some(hit) = hit {
                Box::new(std::iter::once(hit.clone()))
            } else {
                Box::new(std::iter::empty())
            }
        }
    }

    /// Search for files by regular expression.
    ///
    /// This implementation uses the in-memory database to find packages
    /// containing files matching the specified regex pattern.
    ///
    /// # Arguments
    /// * `path` - Regular expression pattern to match against file paths
    /// * `case_insensitive` - Whether to ignore case when matching
    ///
    /// # Returns
    /// Iterator of package names containing matching files
    fn search_files_regex<'a>(
        &'a self,
        path: &str,
        case_insensitive: bool,
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        log::debug!("Searching for {} in {:?}", path, self.0.keys());
        let re = regex::RegexBuilder::new(path)
            .case_insensitive(case_insensitive)
            .build()
            .unwrap();
        Box::new(
            self.0
                .iter()
                .filter(move |(p, _)| re.is_match(p.to_str().unwrap()))
                .map(|(_, pkg)| pkg.to_string()),
        )
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[ignore = "requires network access to deb.debian.org"]
    fn test_remote_contents_resolves_python3_build() {
        use std::str::FromStr;
        // End-to-end: a Debian trixie .sources (no Architectures field) must load
        // Contents (including Contents-all, where arch-independent packages live)
        // and resolve python3-build for the module "build".
        let s =
            "Types: deb\nURIs: http://deb.debian.org/debian\nSuites: trixie\nComponents: main\n";
        let repos = apt_sources::Repositories::from_str(s).unwrap();
        let load_url = |url: &url::Url| load_direct_url(url);
        let urls: Vec<url::Url> = contents_urls_from_sources(
            &repos,
            "amd64",
            load_url,
            crate::debian::apt::verify::trusted_certs_host,
        )
        .collect();
        assert!(!urls.is_empty(), "no Contents URLs built");

        let mut searcher = RemoteContentsFileSearcher { db: HashMap::new() };
        searcher
            .load_urls(urls.into_iter(), load_url, false)
            .unwrap();

        // Query the way production does: the regex used by
        // get_possible_python3_paths_for_python_object for module "build".
        let packages: Vec<String> = searcher
            .search_files_regex(r"/usr/lib/python3/dist\-packages/build/__init__\.py", false)
            .collect();
        assert!(
            packages.iter().any(|p| p == "python3-build"),
            "expected python3-build, got {:?}",
            packages
        );
    }

    #[test]
    fn test_generated_file_searchers() {
        let searchers = &GENERATED_FILE_SEARCHER;
        assert_eq!(
            searchers
                .search_files(Path::new("/etc/locale.gen"), false)
                .collect::<Vec<String>>(),
            vec!["locales"]
        );
        assert_eq!(
            searchers
                .search_files(Path::new("/etc/LOCALE.GEN"), true)
                .collect::<Vec<String>>(),
            vec!["locales"]
        );
        assert_eq!(
            searchers
                .search_files(Path::new("/usr/bin/rst2html"), false)
                .collect::<Vec<String>>(),
            vec!["python3-docutils"]
        );
    }

    #[test]
    fn test_setup_apt_file() {
        use crate::session::unshare::{create_debian_session_for_testing, UnshareSession};

        fn test_session() -> Option<UnshareSession> {
            // Don't run tests if we're in github actions (CI environment restrictions)
            if std::env::var("GITHUB_ACTIONS").is_ok() {
                return None;
            }
            create_debian_session_for_testing("sid", false).ok()
        }

        let session = if let Some(session) = test_session() {
            session
        } else {
            return;
        };

        // Test that setup_apt_file runs without errors
        let result = setup_apt_file(&session);
        assert!(result.is_ok(), "setup_apt_file failed: {:?}", result);

        // Verify apt-file is installed and functional
        let output = session
            .command(vec!["apt-file", "--help"])
            .output()
            .expect("Failed to run apt-file --help");
        assert!(output.status.success(), "apt-file --help failed");

        // Verify apt-file cache exists (Contents files should be downloaded)
        let cache_check = session
            .command(vec!["ls", "/var/cache/apt/apt-file/"])
            .output()
            .expect("Failed to check apt-file cache");
        assert!(
            cache_check.status.success(),
            "apt-file cache directory not found"
        );

        // Test that apt-file can actually search for a file
        let search_result = session
            .command(vec!["apt-file", "search", "bin/ls"])
            .output()
            .expect("Failed to run apt-file search");
        assert!(search_result.status.success(), "apt-file search failed");

        let search_output = String::from_utf8_lossy(&search_result.stdout);
        assert!(
            !search_output.trim().is_empty(),
            "apt-file search returned no results"
        );
        assert!(
            search_output.contains("coreutils"),
            "Expected coreutils package in search results"
        );
    }
}
