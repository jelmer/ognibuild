//! File searching utilities for Debian packages.
//!
//! This module provides functionality for searching files in Debian
//! packages, including using apt-file and other package contents databases.

use crate::debian::sources_list::{SourcesEntry, SourcesList};
use crate::session::{Error as SessionError, Session};
use debian_control::apt::Release;
use flate2::read::GzDecoder;
use lzma_rs::lzma_decompress;
use std::collections::{HashMap, HashSet};
use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use std::path::{Path, PathBuf};
use url::Url;

/// Errors that can occur when searching for files in Debian packages.
#[derive(Debug)]
pub enum Error {
    /// Error accessing apt-file or its cache.
    AptFileAccessError(String),
    /// File not found in the package contents database.
    FileNotFoundError(String),
    /// I/O error accessing files or network resources.
    IoError(std::io::Error),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::IoError(e)
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::AptFileAccessError(e) => write!(f, "AptFileAccessError: {}", e),
            Error::FileNotFoundError(e) => write!(f, "FileNotFoundError: {}", e),
            Error::IoError(e) => write!(f, "IoError: {}", e),
        }
    }
}

impl std::error::Error for Error {}

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

/// Read a Debian contents file.
///
/// Contents files map file paths to package names.
///
/// # Arguments
/// * `f` - Reader for the contents file
///
/// # Returns
/// Iterator of (file path, package name) pairs
pub fn read_contents_file<R: Read>(f: R) -> impl Iterator<Item = (String, String)> {
    BufReader::new(f).lines().map(|line| {
        let line = line.unwrap();
        let (path, rest) = line.rsplit_once(' ').unwrap();
        (path.to_string(), rest.to_string())
    })
}

/// Get URLs for contents files from a sources entry.
///
/// # Arguments
/// * `entry` - Sources entry to get contents URLs from
/// * `arches` - List of architectures to include
/// * `load_url` - Function to load a URL and get a reader
///
/// # Returns
/// Iterator of URLs for contents files
pub fn contents_urls_from_sources_entry<'a>(
    entry: &'a SourcesEntry,
    arches: Vec<&'a str>,
    load_url: impl Fn(&url::Url) -> Result<Box<dyn Read>, Error>,
) -> Box<dyn Iterator<Item = url::Url> + 'a> {
    match entry {
        SourcesEntry::Deb { uri, dist, comps } => {
            let base_url = uri.trim_end_matches('/');
            let name = dist.trim_end_matches('/');
            let dists_url: url::Url = if comps.is_empty() {
                base_url.to_string()
            } else {
                format!("{}/dists", base_url)
            }
            .parse()
            .unwrap();
            let inrelease_url: Url = dists_url.join(&format!("{}/InRelease", name)).unwrap();
            let mut response = match load_url(&inrelease_url) {
                Ok(response) => response,
                Err(_) => {
                    let release_url = dists_url.join(&format!("{}/Release", name)).unwrap();
                    match load_url(&release_url) {
                        Ok(response) => response,
                        Err(e) => {
                            log::warn!(
                                "Unable to download {} or {}: {}",
                                inrelease_url,
                                release_url,
                                e
                            );
                            return Box::new(vec![].into_iter());
                        }
                    }
                }
            };
            let mut release = String::new();
            response.read_to_string(&mut release).unwrap();
            let mut existing_names = HashMap::new();
            let release: Release = release.parse().unwrap();
            for name in release
                .checksums_md5()
                .into_iter()
                .map(|x| x.filename)
                .chain(release.checksums_sha256().into_iter().map(|x| x.filename))
                .chain(release.checksums_sha1().into_iter().map(|x| x.filename))
                .chain(release.checksums_sha512().into_iter().map(|x| x.filename))
            {
                existing_names.insert(
                    std::path::PathBuf::from(name.clone())
                        .file_stem()
                        .unwrap()
                        .to_owned(),
                    name,
                );
            }
            let mut contents_files = HashSet::new();
            if comps.is_empty() {
                for arch in arches {
                    contents_files.insert(format!("Contents-{}", arch));
                }
            } else {
                for comp in comps {
                    for arch in &arches {
                        contents_files.insert(format!("{}/Contents-{}", comp, arch));
                    }
                }
            }
            return Box::new(contents_files.into_iter().filter_map(move |f| {
                if let Some(name) =
                    existing_names.get(&std::path::Path::new(&f).file_stem().unwrap().to_owned())
                {
                    return Some(dists_url.join(name).unwrap().join(&f).unwrap());
                }
                None
            }));
        }
        SourcesEntry::DebSrc { .. } => Box::new(vec![].into_iter()),
    }
}

/// Get URLs for contents files from a sources.list file.
///
/// # Arguments
/// * `sl` - Sources list to get contents URLs from
/// * `arch` - Architecture to include
/// * `load_url` - Function to load a URL and get a reader
///
/// # Returns
/// Iterator of URLs for contents files
pub fn contents_urls_from_sourceslist<'a>(
    sl: &'a SourcesList,
    arch: &'a str,
    load_url: impl Fn(&'_ url::Url) -> Result<Box<dyn Read>, Error> + 'a + Copy,
) -> impl Iterator<Item = url::Url> + 'a {
    // TODO(jelmer): Verify signatures, etc.
    let arches = vec![arch, "all"];
    sl.iter()
        .flat_map(move |source| contents_urls_from_sources_entry(source, arches.clone(), load_url))
}

/// Unwrap a compressed file based on its extension.
///
/// # Arguments
/// * `f` - Reader for the compressed file
/// * `ext` - File extension (e.g., "gz", "xz")
///
/// # Returns
/// Reader for the decompressed contents
pub fn unwrap<'a, R: Read + 'a>(f: R, ext: &str) -> Box<dyn Read + 'a> {
    match ext {
        ".gz" => Box::new(GzDecoder::new(f)),
        ".xz" => {
            let mut compressed_reader = BufReader::new(f);
            let mut decompressed_data = Vec::new();
            lzma_decompress(&mut compressed_reader, &mut decompressed_data).unwrap();
            Box::new(std::io::Cursor::new(decompressed_data.into_iter()))
        }
        ".lz4" => Box::new(lz4_flex::frame::FrameDecoder::new(f)),
        _ => Box::new(f),
    }
}

/// Load a URL directly without caching.
///
/// # Arguments
/// * `url` - URL to load
///
/// # Returns
/// Reader for the URL contents
pub fn load_direct_url(url: &url::Url) -> Result<Box<dyn Read>, Error> {
    for ext in [".xz", ".gz", ""] {
        let response = match reqwest::blocking::get(url.to_string() + ext) {
            Ok(response) => response,
            Err(e) => {
                if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    continue;
                }
                return Err(Error::AptFileAccessError(format!(
                    "Unable to access apt URL {}{}: {}",
                    url, ext, e
                )));
            }
        };
        return Ok(unwrap(response, ext));
    }
    Err(Error::FileNotFoundError(format!("{} not found", url)))
}

/// Load a URL with caching in the specified directories.
///
/// # Arguments
/// * `url` - URL to load
/// * `cache_dirs` - Directories to check for cached content
///
/// # Returns
/// Reader for the URL contents
pub fn load_url_with_cache(url: &url::Url, cache_dirs: &[&Path]) -> Result<Box<dyn Read>, Error> {
    for cache_dir in cache_dirs {
        match load_apt_cache_file(url, cache_dir) {
            Ok(f) => return Ok(Box::new(f)),
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    return Err(e.into());
                }
            }
        }
    }
    load_direct_url(url)
}

/// Convert a URI into a safe filename. It quotes all unsafe characters and converts / to _ and removes the scheme identifier.
pub fn uri_to_filename(url: &url::Url) -> String {
    let mut url = url.clone();
    url.set_username("").unwrap();
    url.set_password(None).unwrap();

    use percent_encoding::{utf8_percent_encode, AsciiSet, CONTROLS};

    // Define the set of characters that need to be percent-encoded
    const BAD_CHARS: &AsciiSet = &CONTROLS
        .add(b' ') // Add space
        .add(b'\"') // Add "
        .add(b'\\') // Add \
        .add(b'{')
        .add(b'}')
        .add(b'[')
        .add(b']')
        .add(b'<')
        .add(b'>')
        .add(b'^')
        .add(b'~')
        .add(b'_')
        .add(b'=')
        .add(b'!')
        .add(b'@')
        .add(b'#')
        .add(b'$')
        .add(b'%')
        .add(b'^')
        .add(b'&')
        .add(b'*');

    let mut u = url.to_string();
    if let Some(pos) = u.find("://") {
        u = u[(pos + 3)..].to_string(); // Remove the scheme
    }

    // Percent-encode the bad characters
    let encoded_uri = utf8_percent_encode(&u, BAD_CHARS).to_string();

    // Replace '/' with '_'
    encoded_uri.replace('/', "_")
}

/// Load a file from the APT cache directory.
///
/// # Arguments
/// * `url` - URL to load
/// * `cache_dir` - APT cache directory
///
/// # Returns
/// Reader for the cached file
pub fn load_apt_cache_file(
    url: &url::Url,
    cache_dir: &Path,
) -> Result<Box<dyn Read>, std::io::Error> {
    let f = uri_to_filename(url);
    for ext in [".xz", ".gz", ".lz4", ""] {
        let p = cache_dir.join([&f, ext].concat());
        if !p.exists() {
            continue;
        }
        log::debug!("Loading cached contents file {}", p.display());
        // return os.popen('/usr/lib/apt/apt-helper cat-file %s' % p)
        let f = File::open(p)?;
        return Ok(unwrap(f, ext));
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", url),
    ))
}

#[allow(missing_docs)]
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
    pub fn from_session(session: &dyn Session) -> AptFileFileSearcher {
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
        Ok(Box::new(RemoteContentsFileSearcher::from_session(session)?)
            as Box<dyn FileSearcher<'a>>)
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
        let sl = SourcesList::default();
        let arch = crate::debian::build::get_build_architecture();
        let cache_dirs = vec![Path::new("/var/lib/apt/lists")];
        let load_url = |url: &url::Url| load_url_with_cache(url, cache_dirs.as_slice());
        let urls = contents_urls_from_sourceslist(&sl, &arch, load_url);
        self.load_urls(urls, load_url)
    }

    /// Load contents information from APT sources configured in a session.
    ///
    /// # Arguments
    /// * `session` - Session for running commands
    ///
    /// # Returns
    /// Ok(()) if successful, Error otherwise
    pub fn load_from_session(&mut self, session: &dyn Session) -> Result<(), Error> {
        // TODO(jelmer): what about sources.list.d?
        let sl = SourcesList::from_apt_dir(&session.external_path(Path::new("/etc/apt")));
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
        let urls = contents_urls_from_sourceslist(&sl, &arch, load_url);
        self.load_urls(urls, load_url)
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
    ) -> Result<(), Error> {
        for url in urls {
            let f = load_url(&url)?;
            self.load_file(f, url);
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
#[allow(missing_docs)]
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
    fn test_uri_to_filename() {
        assert_eq!(
            uri_to_filename(&"http://example.com/foo/bar".parse().unwrap()),
            "example.com_foo_bar"
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
    fn test_unwrap() {
        let data = b"hello world";
        let f = std::io::Cursor::new(data);
        let f = unwrap(f, "");
        let mut buf = Vec::new();
        f.take(5).read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"hello");
    }
}
