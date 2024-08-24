use std::fs::File;
use std::io::{BufRead, BufReader};
use std::path::Path;

#[derive(Debug, PartialEq, Eq)]
pub enum SourcesEntry {
    Deb { uri: String, dist: String, comps: Vec<String> },
    DebSrc { uri: String, dist: String, comps: Vec<String> },
}

pub fn parse_sources_list_entry(line: &str) -> Option<SourcesEntry> {
    let parts = line.split_whitespace().collect::<Vec<_>>();
    if parts.len() < 3 {
        return None;
    }
    let uri = parts[1];
    let dist = parts[2];
    let comps = parts[3..].iter().map(|x| x.to_string()).collect::<Vec<_>>();
    if parts[0] == "deb" {
        return Some(SourcesEntry::Deb { uri: uri.to_string(), dist: dist.to_string(), comps });
    }
    if parts[0] == "deb-src" {
        return Some(SourcesEntry::DebSrc { uri: uri.to_string(), dist: dist.to_string(), comps });
    }
    None
}

pub struct SourcesList {
    list: Vec<SourcesEntry>,
}

impl SourcesList {
    pub fn empty() -> SourcesList {
        SourcesList { list: vec![] }
    }

    pub fn load(&mut self, path: &Path) {
        let f = File::open(path).unwrap();
        for line in BufReader::new(f).lines() {
            let line = line.unwrap();
            if let Some(entry) = parse_sources_list_entry(&line) {
                self.list.push(entry);
            }
        }
    }
}

impl Default for SourcesList {
    fn default() -> Self {
        let mut sl = SourcesList::empty();
        sl.load(Path::new("/etc/apt/sources.list"));
        for path in &["/etc/apt/sources.list.d"] {
            let dir = Path::new(path);
            if dir.exists() && dir.is_dir() {
                for entry in dir.read_dir().unwrap() {
                    let entry = entry.unwrap();
                    if entry.file_type().unwrap().is_file() {
                        let path = entry.path();
                        sl.load(&path);
                    }
                }
            }
        }
        sl
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn test_parse_sources_list_entry() {
        use super::parse_sources_list_entry;
        use super::SourcesEntry;
        assert_eq!(parse_sources_list_entry("deb http://archive.ubuntu.com/ubuntu/ bionic main restricted"), Some(SourcesEntry::Deb { uri: "http://archive.ubuntu.com/ubuntu/".to_string(), dist: "bionic".to_string(), comps: vec!["main".to_string(), "restricted".to_string()] }));
        assert_eq!(parse_sources_list_entry("deb-src http://archive.ubuntu.com/ubuntu/ bionic main restricted"), Some(SourcesEntry::DebSrc { uri: "http://archive.ubuntu.com/ubuntu/".to_string(), dist: "bionic".to_string(), comps: vec!["main".to_string(), "restricted".to_string()] }));
    }

    #[test]
    fn test_sources_list() {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("sources.list");
        std::fs::write(&path, "deb http://archive.ubuntu.com/ubuntu/ bionic main restricted\n").unwrap();
        let mut sl = super::SourcesList::empty();
        sl.load(&path);
        assert_eq!(sl.list.len(), 1);
        assert_eq!(sl.list[0], super::SourcesEntry::Deb { uri: "http://archive.ubuntu.com/ubuntu/".to_string(), dist: "bionic".to_string(), comps: vec!["main".to_string(), "restricted".to_string()] });
    }
}
