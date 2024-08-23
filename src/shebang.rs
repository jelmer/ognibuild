use std::io::BufRead;
use std::os::unix::fs::PermissionsExt;

/// Work out what binary is necessary to run a script based on shebang
///
/// # Arguments
/// * `path` - Path to the script
///
/// # Returns
/// * `Ok(Some(binary))` - The binary necessary to run the script
pub fn shebang_binary(path: &std::path::Path) -> std::io::Result<Option<String>> {
    let file = std::fs::File::open(path)?;
    if file.metadata()?.permissions().mode() & 0o111 == 0 {
        return Ok(None);
    }

    let bufreader = std::io::BufReader::new(file);

    let firstline = bufreader.lines().next();
    let firstline = match firstline {
        Some(line) => line?,
        None => return Ok(None),
    };

    if !firstline.starts_with("#!") {
        return Ok(None);
    }

    let args: Vec<&str> = firstline[2..].split_whitespace().collect();
    let binary = if args[0] == "/usr/bin/env" || args[0] == "env" {
        args[1]
    } else {
        args[0]
    };

    Ok(Some(
        std::path::Path::new(binary)
            .file_name()
            .unwrap()
            .to_string_lossy()
            .to_string(),
    ))
}

#[cfg(test)]
mod tests {
    use super::*;
    fn assert_shebang(content: &str, executable: bool, expected: Option<&str>) {
        let td = tempfile::tempdir().unwrap();
        let path = td.path().join("test.sh");
        std::fs::write(&path, content).unwrap();
        if executable {
            let mut perms = std::fs::metadata(&path).unwrap().permissions();
            perms.set_mode(0o755);
            std::fs::set_permissions(&path, perms).unwrap();
        }
        let binary = super::shebang_binary(&path).unwrap();
        assert_eq!(binary, expected.map(|s| s.to_string()));
    }
    #[test]
    fn test_empty() {
        assert_shebang("", true, None);
    }

    #[test]
    fn test_not_executable() {
        assert_shebang("#!/bin/sh\necho hello", false, None);
    }

    #[test]
    fn test_noshebang_line() {
        assert_shebang("echo hello", true, None);
    }

    #[test]
    fn test_env() {
        assert_shebang("#!/usr/bin/env sh\necho hello", true, Some("sh"));
    }

    #[test]
    fn test_plain() {
        assert_shebang("#!/bin/sh\necho hello", true, Some("sh"));
    }

    #[test]
    fn test_with_arg() {
        assert_shebang("#!/bin/sh -e\necho hello", true, Some("sh"));
    }
}
