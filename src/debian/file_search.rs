use std::fs::File;
use std::io::{BufRead, BufReader, Read};
use crate::session::{Session, Error as SessionError};
use std::path::{Path, PathBuf};
use flate2::read::GzDecoder;
use url::Url;
use std::collections::{HashMap, HashSet};
use lzma_rs::lzma_decompress;
use crate::debian::sources_list::{SourcesList, SourcesEntry};
use debian_control::apt::Release;

#[derive(Debug)]
pub enum Error {
    AptFileAccessError(String),
    FileNotFoundError(String),
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

pub trait FileSearcher<'b> {
    fn search_files<'a>(
        &'a self,
        path: &Path, case_insensitive: bool) -> Box<dyn Iterator<Item = String>+'a>;

    fn search_files_regex<'a>(
        &'a self,
        path: &str, case_insensitive: bool) -> Box<dyn Iterator<Item = String>+'a>;
}

pub fn read_contents_file<R: Read>(f: R) -> impl Iterator<Item = (String, String)> {
    BufReader::new(f)
        .lines()
        .map(|line| {
            let line = line.unwrap();
            let (path, rest) = line.rsplit_once(' ').unwrap();
            (path.to_string(), rest.to_string())
        })
}

pub fn contents_urls_from_sources_entry<'a>(entry: &'a SourcesEntry, arches: Vec<&'a str>, load_url: impl Fn(&url::Url) -> Result<Box<dyn Read>, Error>) -> Box<dyn Iterator<Item = url::Url> + 'a> {
    match entry {
        SourcesEntry::Deb { uri, dist, comps } => {
            let base_url = uri.trim_end_matches('/');
            let name = dist.trim_end_matches('/');
            let dists_url: url::Url = if comps.is_empty() {
                base_url.to_string()
            } else {
                format!("{}/dists", base_url)
            }.parse().unwrap();
            let inrelease_url: Url = dists_url.join(&format!("{}/InRelease", name)).unwrap();
            let mut response = match load_url(&inrelease_url) {
                Ok(response) => response,
                Err(_) => {
                    let release_url = dists_url.join(&format!("{}/Release", name)).unwrap();
                    match load_url(&release_url) {
                        Ok(response) => response,
                        Err(e) => {
                            log::warn!("Unable to download {} or {}: {}", inrelease_url, release_url, e);
                            return Box::new(vec![].into_iter());
                        }
                    }
                }
            };
            let mut release = String::new();
            response.read_to_string(&mut release).unwrap();
            let mut existing_names = HashMap::new();
            let release: Release = release.parse().unwrap();
            for name in release.checksums_md5().into_iter().map(|x| x.filename).chain(release.checksums_sha256().into_iter().map(|x| x.filename)).chain(release.checksums_sha1().into_iter().map(|x| x.filename)).chain(release.checksums_sha512().into_iter().map(|x| x.filename)) {
                existing_names.insert(std::path::PathBuf::from(name.clone()).file_stem().unwrap().to_owned(), name);
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
                if let Some(name) = existing_names.get(&std::path::Path::new(&f).file_stem().unwrap().to_owned()) {
                    return Some(dists_url.join(name).unwrap().join(&f).unwrap());
                }
                None
            }));
        }
        SourcesEntry::DebSrc { .. } => {
            Box::new(vec![].into_iter())
        }
    }
}

pub fn contents_urls_from_sourceslist<'a>(sl: &'a SourcesList, arch: &'a str, load_url: impl Fn(&'_ url::Url) -> Result<Box<dyn Read>, Error> + 'a + Copy) -> impl Iterator<Item = url::Url> + 'a  {
    // TODO(jelmer): Verify signatures, etc.
    let arches = vec![arch, "all"];
    sl.iter().flat_map(move |source| {
        contents_urls_from_sources_entry(source, arches.clone(), load_url)
    })
}

pub fn unwrap<'a, R: Read + 'a>(f: R, ext: &str) -> Box<dyn Read + 'a> {
    match ext {
        ".gz" => Box::new(GzDecoder::new(f)),
        ".xz" => {
            let mut compressed_reader = BufReader::new(f);
            let mut decompressed_data = Vec::new();
            lzma_decompress(&mut compressed_reader, &mut decompressed_data).unwrap();
            Box::new(std::io::Cursor::new(decompressed_data.into_iter()))
        },
        ".lz4" => Box::new(lz4_flex::frame::FrameDecoder::new(f)),
        _ => Box::new(f)
    }
}

pub fn load_direct_url(url: &url::Url) -> Result<Box<dyn Read>, Error> {
    for ext in [".xz", ".gz", ""] {
        let response = match reqwest::blocking::get(url.to_string() + ext) {
            Ok(response) => response,
            Err(e) => {
                if e.status() == Some(reqwest::StatusCode::NOT_FOUND) {
                    continue;
                }
                return Err(Error::AptFileAccessError(
                    format!("Unable to access apt URL {}{}: {}", url, ext, e)
                ));
            }
        };
        return Ok(unwrap(response, ext));
    }
    Err(Error::FileNotFoundError(format!("{} not found", url)))
}

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
        .add(b' ')  // Add space
        .add(b'\"') // Add "
        .add(b'\\') // Add \
        .add(b'{').add(b'}')
        .add(b'[').add(b']')
        .add(b'<').add(b'>')
        .add(b'^').add(b'~')
        .add(b'_').add(b'=')
        .add(b'!').add(b'@')
        .add(b'#').add(b'$')
        .add(b'%').add(b'^')
        .add(b'&').add(b'*');

    let mut u = url.to_string();
    if let Some(pos) = u.find("://") {
        u = u[(pos + 3)..].to_string(); // Remove the scheme
    }

    // Percent-encode the bad characters
    let encoded_uri = utf8_percent_encode(&u, BAD_CHARS).to_string();

    // Replace '/' with '_'
    encoded_uri.replace('/', "_")
}

pub fn load_apt_cache_file(url: &url::Url, cache_dir: &Path) -> Result<Box<dyn Read>, std::io::Error> {
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

lazy_static::lazy_static! {
    pub static ref CACHE_IS_EMPTY_PATH: &'static Path = Path::new("/usr/share/apt-file/is-cache-empty");
}

pub struct AptFileFileSearcher<'a> {
    session: &'a dyn Session
}

impl<'a> AptFileFileSearcher<'a> {
    pub fn has_cache(session: &dyn Session) -> Result<bool, SessionError> {
        if !session.exists(&CACHE_IS_EMPTY_PATH) {
            return Ok(false);
        }
        match session.check_call(vec![&CACHE_IS_EMPTY_PATH.to_str().unwrap()], None, None, None) {
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

    pub fn from_session(session: &dyn Session) -> AptFileFileSearcher {
        log::debug!("Using apt-file to search apt contents");
        if !session.exists(&CACHE_IS_EMPTY_PATH) {
            crate::debian::apt::AptManager::from_session(session).satisfy(vec!["apt-file"]).unwrap();
        }
        if !Self::has_cache(session).unwrap() {
            session.check_call(vec!["apt-file", "update"], None, Some("root"), None).unwrap();
        }
        AptFileFileSearcher { session }
    }

    fn search_files_ex(
        &self, path: &str, regex: bool, case_insensitive: bool
    ) -> Result<impl Iterator<Item = String>, Error> {
        let mut args = vec!["apt-file", "search"];
        if regex {
            args.push("-x");
        } else {
            args.push("-F");
        }
        if case_insensitive {
            args.push("-i");
        }
        args.push(path);
        let output = self.session.check_output(args, None, None, None).unwrap();
        let output_str = std::str::from_utf8(&output).unwrap();
        if output_str == "apt-file: cache is empty\n" {
            return Err(Error::AptFileAccessError("apt-file cache is empty".to_owned()));
        }
        let entries = output_str.split('\n').filter_map(|line| {
            if line.is_empty() {
                return None;
            }
            let (pkg, _path) = line.split_once(": ").unwrap();
            Some(pkg.to_string())
        }).collect::<Vec<String>>();
        Ok(entries.into_iter())
    }
}

impl<'b> FileSearcher<'b> for AptFileFileSearcher<'b> {
    fn search_files<'a>(
        &'a self, path: &Path, case_insensitive: bool
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        return Box::new(self.search_files_ex(path.to_str().unwrap(), false, case_insensitive).unwrap());
    }

    fn search_files_regex<'a>(
        &'a self, path: &str, case_insensitive: bool
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        Box::new(self.search_files_ex(path, true, case_insensitive).unwrap())
    }
}

pub fn get_apt_contents_file_searcher<'a>(session: &'a dyn Session) -> Result<Box<dyn FileSearcher<'a> + 'a>, Error> {
    if AptFileFileSearcher::has_cache(session).unwrap() {
        Ok(Box::new(AptFileFileSearcher::from_session(session)) as Box<dyn FileSearcher<'a>>)
    } else {
        Ok(Box::new(RemoteContentsFileSearcher::from_session(session)?) as Box<dyn FileSearcher<'a>>)
    }
}


pub struct RemoteContentsFileSearcher {
    db: HashMap<String, Vec<u8>>,
}

impl RemoteContentsFileSearcher {
    pub fn from_session(session: &dyn Session) -> Result<RemoteContentsFileSearcher, Error> {
        log::debug!("Loading apt contents information");
        let mut ret = RemoteContentsFileSearcher { db: HashMap::new() };
        ret.load_from_session(session)?;
        Ok(ret)
    }

    pub fn load_local(&mut self) -> Result<(), Error>{
        let sl = SourcesList::default();
        let arch = crate::debian::build::get_build_architecture();
        let cache_dirs = vec![Path::new("/var/lib/apt/lists")];
        let load_url = |url: &url::Url| load_url_with_cache(url, cache_dirs.as_slice());
        let urls = contents_urls_from_sourceslist(&sl, &arch, load_url);
        self.load_urls(urls, load_url)
    }

    pub fn load_from_session(&mut self, session: &dyn Session) -> Result<(), Error>{
        // TODO(jelmer): what about sources.list.d?
        let sl = SourcesList::from_apt_dir(&session.external_path(Path::new("/etc/apt")));
        let arch = crate::debian::build::get_build_architecture();
        let cache_dirs = [session.external_path(Path::new("/var/lib/apt/lists"))];
        let load_url = |url: &url::Url| load_url_with_cache(url, cache_dirs.iter().map(|p| p.as_ref()).collect::<Vec<&Path>>().as_slice());
        let urls = contents_urls_from_sourceslist(&sl, &arch, load_url);
        self.load_urls(urls, load_url)
    }

    fn load_urls(&mut self, urls: impl Iterator<Item = url::Url>, load_url: impl Fn(&url::Url) -> Result<Box<dyn Read>, Error>) -> Result<(), Error> {
        for url in urls {
            let f = load_url(&url)?;
            self.load_file(f, url);
        }
        Ok(())
    }

    pub fn search_files_ex<'a>(
        &'a self, mut matches: impl FnMut(&Path) -> bool + 'a
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        Box::new(self.db.iter().filter(move |(p, _)| matches(Path::new(p))).map(|(_, rest)| {
                std::str::from_utf8(rest.split(|c| *c == b'/').last().unwrap()).unwrap().to_string()
        }))
    }

    fn load_file(&mut self, f: impl Read, url: url::Url) {
        let start_time = std::time::Instant::now();
        for (path, rest) in read_contents_file(f) {
            self.db.insert(path, rest.into());
        }
        log::debug!("Read {} in {:?}", url, start_time.elapsed());
    }
}

impl FileSearcher<'_> for RemoteContentsFileSearcher {
    fn search_files<'a>(
        &'a self, path: &Path, case_insensitive: bool
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

    fn search_files_regex<'a>(
        &'a self, path: &str, case_insensitive: bool
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
pub struct GeneratedFileSearcher {
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
        &'a self, mut matches: impl FnMut(&Path) -> bool + 'a
    ) -> Box<dyn Iterator<Item = String> + 'a> {
        let x = self.db.iter().filter(move |(p, _)| matches(p)).map(|(_, pkg)| pkg.to_string());
        Box::new(x)
    }
}

impl FileSearcher<'_> for GeneratedFileSearcher {
    fn search_files<'a>(
        &'a self, path: &Path, case_insensitive: bool
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

    fn search_files_regex<'a>(
        &'a self, path: &str, case_insensitive: bool
    ) -> Box<dyn Iterator<Item = String> + 'a> {
            let re = regex::RegexBuilder::new(path)
                .case_insensitive(case_insensitive)
                .build()
                .unwrap();
        return self.search_files_ex(move |p| {
            re.is_match(p.to_str().unwrap())
        });
    }
}

// TODO(jelmer): read from a file
lazy_static::lazy_static! {
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
pub fn get_packages_for_paths(
    paths: Vec<&str>, searchers: &[&dyn FileSearcher], regex: bool, case_insensitive: bool
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_uri_to_filename() {
        assert_eq!(uri_to_filename(&"http://example.com/foo/bar".parse().unwrap()), "example.com_foo_bar");
    }

    #[test]
    fn test_generated_file_searchers() {
        let searchers = &GENERATED_FILE_SEARCHER;
        assert_eq!(searchers.search_files(Path::new("/etc/locale.gen"), false).collect::<Vec<String>>(), vec!["locales"]);
        assert_eq!(searchers.search_files(Path::new("/etc/LOCALE.GEN"), true).collect::<Vec<String>>(), vec!["locales"]);
        assert_eq!(searchers.search_files(Path::new("/usr/bin/rst2html"), false).collect::<Vec<String>>(), vec!["python3-docutils"]);
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
