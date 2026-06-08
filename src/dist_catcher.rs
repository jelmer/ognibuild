use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

/// List of supported distribution file extensions.
pub const SUPPORTED_DIST_EXTENSIONS: &[&str] = &[
    ".tar.gz",
    ".tgz",
    ".tar.bz2",
    ".tar.xz",
    ".tar.lzma",
    ".tbz2",
    ".tar",
    ".zip",
];

/// Check if a file has a supported distribution extension.
pub fn supported_dist_file(file: &Path) -> bool {
    let name = match file.file_name().and_then(|n| n.to_str()) {
        Some(name) => name,
        None => return false,
    };
    SUPPORTED_DIST_EXTENSIONS
        .iter()
        .any(|&ext| name.ends_with(ext))
}

/// Utility to detect and collect distribution files created by build systems.
///
/// This monitors directories for new or updated distribution files that appear
/// after a build process runs.
pub struct DistCatcher {
    existing_files: Option<HashMap<PathBuf, HashMap<PathBuf, std::fs::DirEntry>>>,
    directories: Vec<PathBuf>,
    files: std::sync::Mutex<Vec<PathBuf>>,
    start_time: std::time::SystemTime,
}

impl DistCatcher {
    /// Create a new DistCatcher to monitor the specified directories.
    pub fn new(directories: Vec<PathBuf>) -> Self {
        Self {
            directories: directories
                .iter()
                .map(|d| d.canonicalize().unwrap())
                .collect(),
            files: std::sync::Mutex::new(Vec::new()),
            start_time: std::time::SystemTime::now(),
            existing_files: None,
        }
    }

    /// Create a DistCatcher with default directory locations.
    pub fn default(directory: &Path) -> Self {
        Self::new(vec![
            directory.join("dist"),
            directory.to_path_buf(),
            directory.join(".."),
        ])
    }

    /// Initialize the file monitoring process.
    ///
    /// Takes a snapshot of existing files to later detect new or modified files.
    pub fn start(&mut self) {
        self.existing_files = Some(
            self.directories
                .iter()
                .map(|d| {
                    let mut map = HashMap::new();
                    for entry in d.read_dir().unwrap() {
                        let entry = entry.unwrap();
                        map.insert(entry.path(), entry);
                    }
                    (d.clone(), map)
                })
                .collect(),
        );
    }

    /// Search for new or updated distribution files.
    ///
    /// Returns the path to a found file if any.
    pub fn find_files(&self) -> Option<PathBuf> {
        let existing_files = self.existing_files.as_ref().unwrap();
        let mut files = self.files.lock().unwrap();
        for directory in &self.directories {
            let old_files = existing_files.get(directory).unwrap();
            let mut possible_new = Vec::new();
            let mut possible_updated = Vec::new();
            if !directory.is_dir() {
                continue;
            }
            for entry in directory.read_dir().unwrap() {
                let entry = entry.unwrap();
                if !entry.file_type().unwrap().is_file() || !supported_dist_file(&entry.path()) {
                    continue;
                }
                let old_entry = old_files.get(&entry.path());
                if old_entry.is_none() {
                    possible_new.push(entry);
                    continue;
                }
                if entry.metadata().unwrap().modified().unwrap() > self.start_time {
                    possible_updated.push(entry);
                    continue;
                }
            }
            if possible_new.len() == 1 {
                let entry = possible_new[0].path();
                log::info!("Found new tarball {:?} in {:?}", entry, directory);
                files.push(entry.clone());
                return Some(entry);
            } else if possible_new.len() > 1 {
                log::warn!(
                    "Found multiple tarballs {:?} in {:?}",
                    possible_new.iter().map(|e| e.path()).collect::<Vec<_>>(),
                    directory
                );
                files.extend(possible_new.iter().map(|e| e.path()));
                return Some(possible_new[0].path());
            }

            if possible_updated.len() == 1 {
                let entry = possible_updated[0].path();
                log::info!("Found updated tarball {:?} in {:?}", entry, directory);
                files.push(entry.clone());
                return Some(entry);
            }
        }
        None
    }

    /// Copy a single distribution file to the target directory.
    ///
    /// Returns the filename of the copied file if successful.
    pub fn copy_single(&self, target_dir: &Path) -> Result<Option<OsString>, std::io::Error> {
        for path in self.files.lock().unwrap().iter() {
            match std::fs::copy(path, target_dir.join(path.file_name().unwrap())) {
                Ok(_) => return Ok(Some(path.file_name().unwrap().into())),
                Err(e) => {
                    if e.kind() == std::io::ErrorKind::AlreadyExists {
                        continue;
                    }
                    return Err(e);
                }
            }
        }
        log::info!("No tarball created :(");
        Err(std::io::Error::new(
            std::io::ErrorKind::NotFound,
            "No tarball found",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_supported_dist_file() {
        assert!(supported_dist_file(Path::new("foo.tar.gz")));
        assert!(supported_dist_file(Path::new("/tmp/foo.tar.gz")));
        assert!(supported_dist_file(Path::new("/tmp/foo.zip")));
        assert!(supported_dist_file(Path::new("/tmp/foo.tgz")));
        assert!(!supported_dist_file(Path::new("/tmp/foo.txt")));
        assert!(!supported_dist_file(Path::new("/tmp/README")));
    }

    fn touch(path: &Path) {
        std::fs::write(path, b"data").unwrap();
    }

    #[test]
    fn test_find_files_detects_new_tarball() {
        let td = tempfile::tempdir().unwrap();
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();
        let tarball = td.path().join("project-1.0.tar.gz");
        touch(&tarball);

        let found = catcher.find_files();
        assert_eq!(found, Some(tarball.canonicalize().unwrap()));
    }

    #[test]
    fn test_find_files_ignores_unsupported_extension() {
        let td = tempfile::tempdir().unwrap();
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();
        touch(&td.path().join("notes.txt"));

        assert_eq!(catcher.find_files(), None);
    }

    #[test]
    fn test_find_files_ignores_preexisting_file() {
        let td = tempfile::tempdir().unwrap();
        touch(&td.path().join("old.tar.gz"));
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();

        assert_eq!(catcher.find_files(), None);
    }

    #[test]
    fn test_find_files_multiple_new_returns_first_and_records_all() {
        let td = tempfile::tempdir().unwrap();
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();
        touch(&td.path().join("a.tar.gz"));
        touch(&td.path().join("b.tar.gz"));

        let found = catcher.find_files();
        assert!(found.is_some());
        assert_eq!(catcher.files.lock().unwrap().len(), 2);
    }

    #[test]
    fn test_find_files_detects_updated_tarball() {
        let td = tempfile::tempdir().unwrap();
        // Construct first so start_time predates the tarball's mtime.
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let tarball = td.path().join("project-1.0.tar.gz");
        touch(&tarball);
        // The file already exists when we snapshot, so it counts as updated
        // rather than new.
        catcher.start();

        let found = catcher.find_files();
        assert_eq!(found, Some(tarball.canonicalize().unwrap()));
    }

    #[test]
    fn test_copy_single_copies_recorded_file() {
        let td = tempfile::tempdir().unwrap();
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();
        let tarball = td.path().join("project-1.0.tar.gz");
        touch(&tarball);
        catcher.find_files();

        let target = tempfile::tempdir().unwrap();
        let copied = catcher.copy_single(target.path()).unwrap();
        assert_eq!(copied, Some(OsString::from("project-1.0.tar.gz")));
        assert!(target.path().join("project-1.0.tar.gz").exists());
    }

    #[test]
    fn test_copy_single_no_files_is_not_found() {
        let td = tempfile::tempdir().unwrap();
        let mut catcher = DistCatcher::new(vec![td.path().to_path_buf()]);
        catcher.start();

        let target = tempfile::tempdir().unwrap();
        let err = catcher.copy_single(target.path()).unwrap_err();
        assert_eq!(err.kind(), std::io::ErrorKind::NotFound);
    }
}
