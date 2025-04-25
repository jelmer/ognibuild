use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

/// Entry in a Debian APT sources.list file.
///
/// This enum represents the two types of entries that can appear in a
/// sources.list file: 'deb' for binary packages and 'deb-src' for source packages.
#[derive(Debug, PartialEq, Eq)]
pub enum SourcesEntry {
    /// Binary package repository entry (deb line).
    Deb {
        /// Repository URI
        uri: String,
        /// Distribution name (e.g., "stable", "bullseye")
        dist: String,
        /// Component names (e.g., "main", "contrib", "non-free")
        comps: Vec<String>,
    },
    /// Source package repository entry (deb-src line).
    DebSrc {
        /// Repository URI
        uri: String,
        /// Distribution name (e.g., "stable", "bullseye")
        dist: String,
        /// Component names (e.g., "main", "contrib", "non-free")
        comps: Vec<String>,
    },
}

/// Parse a line from a sources.list file into a SourcesEntry.
///
/// # Arguments
/// * `line` - Line from sources.list to parse
///
/// # Returns
/// Some(SourcesEntry) if the line is a valid deb or deb-src line,
/// None otherwise (e.g., comments, blank lines, invalid syntax)
pub fn parse_sources_list_entry(line: &str) -> Option<SourcesEntry> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let uri = parts[1];
    let dist = parts[2];
    let comps = parts[3..].iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if parts[0] == "deb" {
        return Some(SourcesEntry::Deb {
            uri: uri.to_string(),
            dist: dist.to_string(),
            comps,
        });
    }
    if parts[0] == "deb-src" {
        return Some(SourcesEntry::DebSrc {
            uri: uri.to_string(),
            dist: dist.to_string(),
            comps,
        });
    }
    None
}

/// Representation of a Debian APT sources.list file.
///
/// This struct holds a collection of SourcesEntry objects, representing
/// the contents of one or more sources.list files.
pub struct SourcesList {
    /// List of sources entries
    list: Vec<SourcesEntry>,
}

impl SourcesList {
    /// Create an empty sources list.
    ///
    /// # Returns
    /// A new SourcesList with no entries
    pub fn empty() -> SourcesList {
        SourcesList { list: vec![] }
    }

    /// Get an iterator over the entries in this sources list.
    ///
    /// # Returns
    /// An iterator over references to SourcesEntry objects
    pub fn iter(&self) -> std::slice::Iter<SourcesEntry> {
        self.list.iter()
    }

    /// Load sources entries from a file.
    ///
    /// # Arguments
    /// * `path` - Path to the sources.list file to load
    pub fn load(&mut self, path: &Path) {
        let f = File::open(path).unwrap();
        for line in BufReader::new(f).lines() {
            let line = line.unwrap();
            if let Some(entry) = parse_sources_list_entry(&line) {
                self.list.push(entry);
            }
        }
    }

    /// Create a SourcesList from an APT directory.
    ///
    /// This loads both the main sources.list file and any additional files
    /// in the sources.list.d directory.
    ///
    /// # Arguments
    /// * `apt_dir` - Path to the APT configuration directory (usually /etc/apt)
    ///
    /// # Returns
    /// A new SourcesList containing entries from all sources files
    pub fn from_apt_dir(apt_dir: &Path) -> SourcesList {
        let mut sl = SourcesList::empty();
        sl.load(&apt_dir.join("sources.list"));
        for entry in apt_dir.read_dir().unwrap() {
            let entry = entry.unwrap();
            if entry.file_type().unwrap().is_file() {
                let path = entry.path();
                sl.load(&path);
            }
        }
        sl
    }
}

impl Default for SourcesList {
    fn default() -> Self {
        Self::from_apt_dir(Path::new("/etc/apt"))
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_sources_list_entry() {
        use super::parse_sources_list_entry;
        use super::SourcesEntry;
        assert_eq!(
            parse_sources_list_entry(
                "deb http://archive.ubuntu.com/ubuntu/ bionic main restricted"
            ),
            Some(SourcesEntry::Deb {
                uri: "http://archive.ubuntu.com/ubuntu/".to_string(),
                dist: "bionic".to_string(),
                comps: vec!["main".to_string(), "restricted".to_string()]
            })
        );
        assert_eq!(
            parse_sources_list_entry(
                "deb-src http://archive.ubuntu.com/ubuntu/ bionic main restricted"
            ),
            Some(SourcesEntry::DebSrc {
                uri: "http://archive.ubuntu.com/ubuntu/".to_string(),
                dist: "bionic".to_string(),
                comps: vec!["main".to_string(), "restricted".to_string()]
            })
        );
    }

    #[test]
    fn test_sources_list() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("sources.list");
        std::fs::write(
            &path,
            "deb http://archive.ubuntu.com/ubuntu/ bionic main restricted\n",
        )
        .unwrap();
        let mut sl = super::SourcesList::empty();
        sl.load(&path);
        assert_eq!(sl.list.len(), 1);
        assert_eq!(
            sl.list[0],
            super::SourcesEntry::Deb {
                uri: "http://archive.ubuntu.com/ubuntu/".to_string(),
                dist: "bionic".to_string(),
                comps: vec!["main".to_string(), "restricted".to_string()]
            }
        );
    }
}
