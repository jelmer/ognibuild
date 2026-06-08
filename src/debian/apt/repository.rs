//! Deriving Contents-file URLs from APT repository metadata.
//!
//! Given a set of `.sources`/`.list` repositories, this module locates and
//! verifies their `Release` files and derives the URLs of the `Contents-*`
//! indices that map file paths to packages. It also handles fetching and
//! caching those indices and decompressing them.

use apt_sources::{
    error::{LoadError, RepositoryError},
    Repository, RepositoryType,
};
use debian_control::apt::Release;
use flate2::read::MultiGzDecoder;
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
    /// Decompression error when unpacking compressed files.
    DecompressionError(String),
}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Error {
        Error::IoError(e)
    }
}

impl From<RepositoryError> for Error {
    fn from(e: RepositoryError) -> Error {
        Error::AptFileAccessError(format!("Repository error: {}", e))
    }
}

impl From<LoadError> for Error {
    fn from(e: LoadError) -> Error {
        Error::AptFileAccessError(format!("Load error: {}", e))
    }
}

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Error::AptFileAccessError(e) => write!(f, "AptFileAccessError: {}", e),
            Error::FileNotFoundError(e) => write!(f, "FileNotFoundError: {}", e),
            Error::IoError(e) => write!(f, "IoError: {}", e),
            Error::DecompressionError(e) => write!(f, "DecompressionError: {}", e),
        }
    }
}

impl std::error::Error for Error {}

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
    let mut reader = BufReader::new(f);
    std::iter::from_fn(move || loop {
        let mut buf = Vec::new();
        match reader.read_until(b'\n', &mut buf) {
            Ok(0) => return None,
            Ok(_) => {}
            Err(e) => panic!("io error reading contents file: {e}"),
        }
        // Contents files are mostly UTF-8 but can contain stray non-UTF-8 bytes
        // in file paths; decode lossily rather than failing the whole index.
        let line = String::from_utf8_lossy(&buf);
        let line = line.trim_end_matches(['\r', '\n']);
        // Each line is "<path><whitespace><section>/<package>[,...]". The path
        // column is space-padded, so split on the final whitespace run and trim.
        let Some(split) = line.rfind(char::is_whitespace) else {
            continue;
        };
        let (path, rest) = line.split_at(split);
        let path = path.trim_end();
        let rest = rest.trim_start();
        if path.is_empty() || rest.is_empty() {
            continue;
        }
        // apt-file (and the regexes that query this DB) use absolute paths,
        // but Contents files list paths without a leading slash.
        return Some((format!("/{path}"), rest.to_string()));
    })
}

/// Get URLs for contents files from a repository entry.
///
/// The repository's `InRelease`/`Release` file is verified against the keys the
/// repository is trusted with (its `Signed-By`, or APT's default trusted
/// keyrings) before any Contents URL is derived from it. A repository whose
/// Release cannot be verified yields no Contents URLs.
///
/// # Arguments
/// * `repo` - Repository to get contents URLs from
/// * `arches` - List of architectures to include
/// * `load_url` - Function to load a URL and get a reader
/// * `resolve_certs` - Resolves the certificates a repository is trusted
///   against, given its `Signed-By` value (`None` for APT's defaults)
///
/// # Returns
/// Iterator of URLs for contents files
pub fn contents_urls_from_repository<'a>(
    repo: &'a Repository,
    arches: Vec<&'a str>,
    load_url: impl Fn(&url::Url) -> Result<Box<dyn Read>, Error>,
    resolve_certs: impl Fn(
        Option<&apt_sources::signature::Signature>,
    ) -> Result<Vec<sequoia_openpgp::Cert>, crate::debian::apt::verify::Error>,
) -> Box<dyn Iterator<Item = url::Url> + 'a> {
    // Only process binary repositories (deb), not source repositories (deb-src)
    if !repo.types.contains(&RepositoryType::Binary) {
        return Box::new(vec![].into_iter());
    }

    // Process all URIs and suites combinations
    let mut all_urls = Vec::new();

    for uri in &repo.uris {
        for dist in &repo.suites {
            let comps = repo
                .components
                .as_ref()
                .map(|c| c.as_slice())
                .unwrap_or(&[]);
            let base_url = uri.as_str().trim_end_matches('/');
            let name = dist.trim_end_matches('/');
            // URL of the dist directory (with a trailing slash so that joining a
            // Release-relative path keeps the directory prefix rather than
            // replacing the last segment). For a flat repository (no components)
            // this is the suite directory directly under the base URL;
            // otherwise it is below dists/.
            let dist_url: url::Url = if comps.is_empty() {
                format!("{}/{}/", base_url, name)
            } else {
                format!("{}/dists/{}/", base_url, name)
            }
            .parse()
            .unwrap();
            // Resolve the keys this repository is trusted against before
            // touching any Release file: a Release we cannot verify must not
            // contribute any Contents URL.
            let certs = match resolve_certs(repo.signature.as_ref()) {
                Ok(certs) => certs,
                Err(e) => {
                    log::error!(
                        "Refusing to use APT Release for {}: no trusted keys: {}",
                        dist_url,
                        e
                    );
                    return Box::new(vec![].into_iter());
                }
            };
            // Prefer the clearsigned InRelease; fall back to a detached
            // Release + Release.gpg. In both cases we parse only the bytes
            // whose signature verified, never the raw download.
            let inrelease_url: Url = dist_url.join("InRelease").unwrap();
            let release = match load_url(&inrelease_url) {
                Ok(mut response) => {
                    let mut signed = Vec::new();
                    response.read_to_end(&mut signed).unwrap();
                    match crate::debian::apt::verify::verify_clearsigned(&signed, certs) {
                        Ok(payload) => String::from_utf8_lossy(&payload).into_owned(),
                        Err(e) => {
                            log::error!(
                                "APT Release signature verification failed for {}: {}",
                                inrelease_url,
                                e
                            );
                            return Box::new(vec![].into_iter());
                        }
                    }
                }
                Err(_) => {
                    let release_url = dist_url.join("Release").unwrap();
                    let signature_url = dist_url.join("Release.gpg").unwrap();
                    let mut release_bytes = Vec::new();
                    let mut signature_bytes = Vec::new();
                    let loaded = load_url(&release_url).and_then(|mut r| {
                        r.read_to_end(&mut release_bytes)?;
                        let mut s = load_url(&signature_url)?;
                        s.read_to_end(&mut signature_bytes)?;
                        Ok(())
                    });
                    if let Err(e) = loaded {
                        log::warn!(
                            "Unable to download {} or {} (+ Release.gpg): {}",
                            inrelease_url,
                            release_url,
                            e
                        );
                        return Box::new(vec![].into_iter());
                    }
                    if let Err(e) = crate::debian::apt::verify::verify_detached(
                        &release_bytes,
                        &signature_bytes,
                        certs,
                    ) {
                        log::error!(
                            "APT Release signature verification failed for {}: {}",
                            release_url,
                            e
                        );
                        return Box::new(vec![].into_iter());
                    }
                    String::from_utf8_lossy(&release_bytes).into_owned()
                }
            };
            // Map each Release-listed Contents file by its dist-relative path
            // with any compression extension stripped (e.g.
            // "main/Contents-amd64") to the actual filename
            // ("main/Contents-amd64.gz"). Keying by the full component-qualified
            // path (rather than just the basename) keeps Contents files of
            // different components distinct -- they share a basename like
            // "Contents-all" but must not collide.
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
                let key = match name.rsplit_once('.') {
                    Some((stem, "gz" | "xz" | "lz4")) => stem.to_string(),
                    _ => name.clone(),
                };
                existing_names.insert(key, name);
            }
            let mut contents_files = HashSet::new();
            if comps.is_empty() {
                for arch in &arches {
                    contents_files.insert(format!("Contents-{}", arch));
                }
            } else {
                for comp in comps {
                    for arch in &arches {
                        contents_files.insert(format!("{}/Contents-{}", comp, arch));
                    }
                }
            }
            let urls: Vec<_> = contents_files
                .into_iter()
                .filter_map(|f| {
                    // Only emit URLs for Contents files the Release actually
                    // lists. existing_names is keyed by the dist-relative path
                    // without a compression extension (e.g. "main/Contents-amd64").
                    // Emit that extension-less URL: load_direct_url probes the
                    // .xz/.gz/"" variants itself and decompresses accordingly.
                    if !existing_names.contains_key(&f) {
                        return None;
                    }
                    Some(dist_url.join(&f).unwrap())
                })
                .collect();
            all_urls.extend(urls);
        }
    }

    Box::new(all_urls.into_iter())
}

/// Get URLs for contents files from APT sources.
///
/// Each repository's Release file is verified against the keys it is trusted
/// with before any Contents URL is derived from it (see
/// [`contents_urls_from_repository`]).
///
/// # Arguments
/// * `repositories` - Repositories to get contents URLs from
/// * `arch` - Architecture to include
/// * `load_url` - Function to load a URL and get a reader
/// * `resolve_certs` - Resolves the certificates a repository is trusted
///   against, given its `Signed-By` value (`None` for APT's defaults)
///
/// # Returns
/// Iterator of URLs for contents files
pub fn contents_urls_from_sources<'a>(
    repositories: &'a apt_sources::Repositories,
    arch: &'a str,
    load_url: impl Fn(&'_ url::Url) -> Result<Box<dyn Read>, Error> + 'a + Copy,
    resolve_certs: impl Fn(
            Option<&apt_sources::signature::Signature>,
        ) -> Result<Vec<sequoia_openpgp::Cert>, crate::debian::apt::verify::Error>
        + 'a
        + Copy,
) -> impl Iterator<Item = url::Url> + 'a {
    let arches = vec![arch, "all"];
    repositories.iter().flat_map(move |repo| {
        contents_urls_from_repository(repo, arches.clone(), load_url, resolve_certs)
    })
}

/// Unwrap a compressed file based on its extension.
///
/// # Arguments
/// * `f` - Reader for the compressed file
/// * `ext` - File extension (e.g., "gz", "xz")
///
/// # Returns
/// Reader for the decompressed contents
pub fn unwrap<'a, R: Read + 'a>(f: R, ext: &str) -> Result<Box<dyn Read + 'a>, Error> {
    match ext {
        // Debian Contents-*.gz are often concatenated multi-member gzip
        // streams; a single-member decoder silently stops after the first.
        ".gz" => Ok(Box::new(MultiGzDecoder::new(f))),
        ".xz" => {
            let mut compressed_reader = BufReader::new(f);
            let mut decompressed_data = Vec::new();
            lzma_decompress(&mut compressed_reader, &mut decompressed_data).map_err(|e| {
                Error::DecompressionError(format!("LZMA decompression failed: {}", e))
            })?;
            Ok(Box::new(std::io::Cursor::new(decompressed_data)))
        }
        ".lz4" => Ok(Box::new(lz4_flex::frame::FrameDecoder::new(f))),
        _ => Ok(Box::new(f)),
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
    // Create a client with reasonable timeouts for downloading large APT Contents files
    let client = reqwest::blocking::Client::builder()
        .timeout(std::time::Duration::from_secs(300)) // 5 minutes for large files
        .connect_timeout(std::time::Duration::from_secs(30)) // 30 seconds to establish connection
        .build()
        .map_err(|e| Error::AptFileAccessError(format!("Failed to create HTTP client: {}", e)))?;

    for ext in [".xz", ".gz", ""] {
        let response = match client.get(url.to_string() + ext).send() {
            Ok(response) => response,
            Err(e) => {
                log::debug!("Failed to fetch APT contents from {}{}: {}", url, ext, e);
                return Err(Error::AptFileAccessError(format!(
                    "Unable to access apt URL {}{}: {}",
                    url, ext, e
                )));
            }
        };
        // A missing compressed variant comes back as a 404 response (not a
        // transport error), so check the status before handing the body to the
        // decompressor -- otherwise the error page is fed to gzip/xz and fails.
        if response.status() == reqwest::StatusCode::NOT_FOUND {
            continue;
        }
        if !response.status().is_success() {
            return Err(Error::AptFileAccessError(format!(
                "Unable to access apt URL {}{}: HTTP {}",
                url,
                ext,
                response.status()
            )));
        }
        return unwrap(response, ext);
    }
    Err(Error::FileNotFoundError(format!("{} not found", url)))
}

/// Get the user cache directory for ognibuild APT Contents files.
fn get_user_cache_dir() -> Option<PathBuf> {
    dirs::cache_dir().map(|d| d.join("ognibuild").join("apt-contents"))
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
    // First check system cache directories
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

    // Then check user cache directory
    if let Some(user_cache_dir) = get_user_cache_dir() {
        match load_apt_cache_file(url, &user_cache_dir) {
            Ok(f) => {
                log::debug!(
                    "Found cached APT contents in user cache: {}",
                    user_cache_dir.display()
                );
                return Ok(Box::new(f));
            }
            Err(e) => {
                if e.kind() != std::io::ErrorKind::NotFound {
                    log::debug!("Error reading from user cache: {}", e);
                }
            }
        }
    }

    // If not found in any cache, download and cache it
    download_and_cache_url(url)
}

/// Download a URL and cache it in the user cache directory.
fn download_and_cache_url(url: &url::Url) -> Result<Box<dyn Read>, Error> {
    // Download the file
    let content = load_direct_url(url)?;

    // Try to cache it in user directory
    if let Some(user_cache_dir) = get_user_cache_dir() {
        // Ensure cache directory exists
        if let Err(e) = std::fs::create_dir_all(&user_cache_dir) {
            log::debug!(
                "Failed to create cache directory {}: {}",
                user_cache_dir.display(),
                e
            );
        } else {
            // Read the content into memory so we can both cache and return it
            let mut buffer = Vec::new();
            let mut reader = content;
            if let Err(e) = std::io::Read::read_to_end(&mut reader, &mut buffer) {
                log::debug!("Failed to read content for caching: {}", e);
                return Ok(reader); // Return the original reader if we can't cache
            }

            // Write to cache file
            let cache_file_path = user_cache_dir.join(uri_to_filename(url));
            match std::fs::write(&cache_file_path, &buffer) {
                Ok(_) => {
                    log::info!("Cached APT contents to: {}", cache_file_path.display());
                }
                Err(e) => {
                    log::debug!(
                        "Failed to write cache file {}: {}",
                        cache_file_path.display(),
                        e
                    );
                }
            }

            // Return the buffer as a reader
            return Ok(Box::new(std::io::Cursor::new(buffer)));
        }
    }

    // If we can't cache, just return the downloaded content
    Ok(content)
}

/// Convert a URI into a safe filename. It quotes all unsafe characters and converts / to _ and removes the scheme identifier.
pub fn uri_to_filename(url: &url::Url) -> String {
    let mut url = url.clone();
    // Strip any credentials from the URL. These setters return Err for URLs
    // that cannot carry credentials (e.g. host-less file:/cdrom: URIs), in
    // which case there is nothing to strip.
    let _ = url.set_username("");
    let _ = url.set_password(None);

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
        return unwrap(f, ext).map_err(|e| match e {
            Error::IoError(io_err) => io_err,
            Error::DecompressionError(msg) => {
                std::io::Error::new(std::io::ErrorKind::InvalidData, msg)
            }
            other => std::io::Error::new(std::io::ErrorKind::Other, format!("{:?}", other)),
        });
    }
    Err(std::io::Error::new(
        std::io::ErrorKind::NotFound,
        format!("{} not found", url),
    ))
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
    fn test_uri_to_filename_with_credentials() {
        assert_eq!(
            uri_to_filename(&"http://user:pass@example.com/foo/bar".parse().unwrap()),
            "example.com_foo_bar"
        );
    }

    #[test]
    fn test_uri_to_filename_host_less() {
        // Host-less URIs (e.g. file:/cdrom:) cannot carry credentials; stripping
        // them must not panic.
        assert_eq!(
            uri_to_filename(&"file:///var/lib/foo/dists/sid/InRelease".parse().unwrap()),
            "_var_lib_foo_dists_sid_InRelease"
        );
    }

    #[test]
    fn test_read_contents_file() {
        // A Contents file column is space-padded; the path gets a leading slash
        // so it matches the absolute-path regexes that query the database.
        let data = b"usr/bin/foo       admin/foo-tools\nusr/lib/bar.so   libs/libbar\n";
        let entries: Vec<(String, String)> =
            read_contents_file(std::io::Cursor::new(data.to_vec())).collect();
        assert_eq!(
            entries,
            vec![
                ("/usr/bin/foo".to_string(), "admin/foo-tools".to_string()),
                ("/usr/lib/bar.so".to_string(), "libs/libbar".to_string()),
            ]
        );
    }

    #[test]
    fn test_read_contents_file_skips_blank_and_headerless_lines() {
        // Blank lines and a path-only line (no package column) are skipped.
        let data = b"\nusr/bin/foo  admin/foo\n   \nleftover\n";
        let entries: Vec<(String, String)> =
            read_contents_file(std::io::Cursor::new(data.to_vec())).collect();
        assert_eq!(
            entries,
            vec![("/usr/bin/foo".to_string(), "admin/foo".to_string())]
        );
    }

    #[test]
    fn test_read_contents_file_tolerates_non_utf8() {
        // A stray non-UTF-8 byte in a path must not abort the whole index; the
        // byte is replaced and the rest of the line still parses.
        let mut data = Vec::new();
        data.extend_from_slice(b"usr/share/\xff/file  doc/weird\n");
        data.extend_from_slice(b"usr/bin/ok  admin/ok\n");
        let entries: Vec<(String, String)> =
            read_contents_file(std::io::Cursor::new(data)).collect();
        assert_eq!(entries.len(), 2);
        assert_eq!(entries[0].1, "doc/weird");
        assert!(entries[0].0.starts_with("/usr/share/"));
        assert_eq!(
            entries[1],
            ("/usr/bin/ok".to_string(), "admin/ok".to_string())
        );
    }

    #[test]
    fn test_unwrap_multi_member_gz() {
        // Debian Contents-*.gz are concatenated multi-member gzip streams; the
        // decoder must read past the first member, not stop after it.
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;
        let mut compressed = Vec::new();
        for part in [b"first member\n".as_ref(), b"second member\n".as_ref()] {
            let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
            encoder.write_all(part).unwrap();
            compressed.extend(encoder.finish().unwrap());
        }
        let mut f = unwrap(std::io::Cursor::new(compressed), ".gz").unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"first member\nsecond member\n");
    }

    fn clearsign_release(release: &str) -> (Vec<u8>, sequoia_openpgp::Cert) {
        use sequoia_openpgp::cert::CertBuilder;
        use sequoia_openpgp::policy::StandardPolicy;
        use sequoia_openpgp::serialize::stream::{Message, Signer};
        use std::io::Write;
        let (cert, _) = CertBuilder::new().add_signing_subkey().generate().unwrap();
        let policy = StandardPolicy::new();
        let keypair = cert
            .keys()
            .with_policy(&policy, None)
            .secret()
            .for_signing()
            .next()
            .unwrap()
            .key()
            .clone()
            .into_keypair()
            .unwrap();
        let mut sink = Vec::new();
        {
            // The cleartext signer produces its own armor framing.
            let message = Message::new(&mut sink);
            let mut signer = Signer::new(message, keypair)
                .unwrap()
                .cleartext()
                .build()
                .unwrap();
            signer.write_all(release.as_bytes()).unwrap();
            signer.finalize().unwrap();
        }
        (sink, cert)
    }

    #[test]
    fn test_contents_urls_from_sources() {
        use std::str::FromStr;
        // A deb822 source without an Architectures field, as shipped by Debian.
        let s =
            "Types: deb\nURIs: http://deb.debian.org/debian\nSuites: trixie\nComponents: main\n";
        let repos = apt_sources::Repositories::from_str(s).unwrap();
        // The Release file lists Contents files relative to the dist directory,
        // including the arch-independent Contents-all.
        let release = concat!(
            "Origin: Debian\n",
            "MD5Sum:\n",
            " 0000000000000000000000000000000a 1 main/Contents-amd64.gz\n",
            " 0000000000000000000000000000000b 1 main/Contents-all.gz\n",
        );
        // Serve a clearsigned InRelease and trust the key it was signed with, so
        // verification succeeds before any URL is derived.
        let (inrelease, cert) = clearsign_release(release);
        let load_url = move |url: &url::Url| -> Result<Box<dyn Read>, Error> {
            if url.path().ends_with("/InRelease") {
                Ok(Box::new(std::io::Cursor::new(inrelease.clone())))
            } else {
                Err(Error::FileNotFoundError(url.to_string()))
            }
        };
        let resolve_certs =
            move |_signature: Option<&apt_sources::signature::Signature>| Ok(vec![cert.clone()]);
        let mut urls: Vec<String> =
            contents_urls_from_sources(&repos, "amd64", &load_url, &resolve_certs)
                .map(|u| u.to_string())
                .collect();
        urls.sort();
        // The compression extension is stripped: load_direct_url probes the
        // .xz/.gz/"" variants and decompresses based on which one it finds.
        assert_eq!(
            urls,
            vec![
                "http://deb.debian.org/debian/dists/trixie/main/Contents-all".to_string(),
                "http://deb.debian.org/debian/dists/trixie/main/Contents-amd64".to_string(),
            ]
        );
    }

    #[test]
    fn test_contents_urls_rejects_untrusted_release() {
        use std::str::FromStr;
        // A Release signed with one key but verified against a different key
        // must yield no Contents URLs.
        let s =
            "Types: deb\nURIs: http://deb.debian.org/debian\nSuites: trixie\nComponents: main\n";
        let repos = apt_sources::Repositories::from_str(s).unwrap();
        let release =
            "Origin: Debian\nMD5Sum:\n 0000000000000000000000000000000a 1 main/Contents-amd64.gz\n";
        let (inrelease, _signing_cert) = clearsign_release(release);
        // A different, untrusted key.
        let (_, other_cert) = clearsign_release("unrelated");
        let load_url = move |url: &url::Url| -> Result<Box<dyn Read>, Error> {
            if url.path().ends_with("/InRelease") {
                Ok(Box::new(std::io::Cursor::new(inrelease.clone())))
            } else {
                Err(Error::FileNotFoundError(url.to_string()))
            }
        };
        let resolve_certs = move |_signature: Option<&apt_sources::signature::Signature>| {
            Ok(vec![other_cert.clone()])
        };
        let urls: Vec<url::Url> =
            contents_urls_from_sources(&repos, "amd64", &load_url, &resolve_certs).collect();
        assert!(urls.is_empty(), "expected no URLs, got {:?}", urls);
    }

    #[test]
    fn test_unwrap_plain() {
        let data = b"hello world";
        let f = std::io::Cursor::new(data);
        let mut f = unwrap(f, "").unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, b"hello world");
    }

    #[test]
    fn test_unwrap_gz() {
        use flate2::write::GzEncoder;
        use flate2::Compression;
        use std::io::Write;

        let original = b"hello world from gzip";
        let mut encoder = GzEncoder::new(Vec::new(), Compression::default());
        encoder.write_all(original).unwrap();
        let compressed = encoder.finish().unwrap();

        let f = std::io::Cursor::new(compressed);
        let mut f = unwrap(f, ".gz").unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, original);
    }

    #[test]
    fn test_unwrap_xz() {
        use lzma_rs::lzma_compress;

        let original = b"hello world from xz";
        let mut compressed = Vec::new();
        lzma_compress(&mut original.as_ref(), &mut compressed).unwrap();

        let f = std::io::Cursor::new(compressed);
        let mut f = unwrap(f, ".xz").unwrap();
        let mut buf = Vec::new();
        f.read_to_end(&mut buf).unwrap();
        assert_eq!(buf, original);
    }

    #[test]
    fn test_unwrap_corrupt_xz() {
        // Test that corrupt XZ data returns an error, not a panic
        let corrupt_data = b"this is not valid xz data";
        let f = std::io::Cursor::new(corrupt_data);
        let result = unwrap(f, ".xz");
        assert!(result.is_err());
        if let Err(Error::DecompressionError(msg)) = result {
            assert!(msg.contains("LZMA"));
        } else {
            panic!("Expected DecompressionError");
        }
    }
}
