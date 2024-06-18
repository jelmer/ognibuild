use std::io::BufRead;
use std::os::unix::fs::PermissionsExt;

pub mod fix_build;
pub mod session;
pub mod vcs;

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
