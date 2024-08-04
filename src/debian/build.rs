use breezyshim::tree::{Tree, MutableTree, WorkingTree};
use debian_changelog::{Entry as ChangelogEntry, ChangeLog};

pub fn get_build_architecture() -> String {
    std::process::Command::new("dpkg-architecture")
        .arg("-qDEB_BUILD_ARCH")
        .output()
        .map(|output| {
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .to_string()
        })
        .unwrap()
}

pub const DEFAULT_BUILDER: &str = "sbuild --no-clean-source";

fn python_command() -> String {
        pyo3::Python::with_gil(|py| {
            use pyo3::types::PyAnyMethods;
            let sys_module = py.import_bound("sys").unwrap();
            sys_module.getattr("executable").unwrap().extract::<String>().unwrap()
        })
}


pub fn builddeb_command(
    build_command: Option<&str>,
    result_dir: Option<&std::path::Path>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<Vec<&str>>,
) -> Vec<String> {
    let mut build_command = build_command.unwrap_or(DEFAULT_BUILDER).to_string();

    if let Some(extra_repositories) = extra_repositories {
        for repo in extra_repositories {
            build_command.push_str(&format!(" --extra-repository={}", shlex::try_quote(repo).unwrap()));
        }
    }

    let mut args = vec![
        python_command(),
        "-m".to_string(),
        "breezy".to_string(),
        "builddeb".to_string(),
        "--guess-upstream-branch-url".to_string(),
        format!("--builder={}", build_command)
    ];

    if let Some(apt_repository) = apt_repository {
        args.push(format!("--apt-repository={}", apt_repository));
    }
    if let Some(apt_repository_key) = apt_repository_key {
        args.push(format!("--apt-repository-key={}", apt_repository_key));
    }
    if let Some(result_dir) = result_dir {
        args.push(format!("--result-dir={}", result_dir.to_string_lossy()));
    }

    args
}

#[derive(Debug)]
pub struct BuildFailedError;

impl std::fmt::Display for BuildFailedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Build failed.")
    }
}

impl std::error::Error for BuildFailedError {}

pub fn build(
    local_tree: &WorkingTree,
    outf: std::fs::File,
    build_command: Option<&str>,
    result_dir: Option<&std::path::Path>,
    distribution: Option<&str>,
    subpath: Option<&std::path::Path>,
    source_date_epoch: Option<chrono::DateTime<chrono::Utc>>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<Vec<&str>>
) -> Result<(), BuildFailedError>{ 
    let subpath = subpath.unwrap_or_else(|| std::path::Path::new(""));
    let build_command = build_command.unwrap_or(DEFAULT_BUILDER);
    let args = builddeb_command(
        Some(build_command),
        result_dir,
        apt_repository,
        apt_repository_key,
        extra_repositories,
    );

    // Make a copy of the environment variables
    let mut env = std::env::vars().collect::<std::collections::HashMap<_, _>>();

    if let Some(distribution) = distribution {
        env.insert("DISTRIBUTION".to_owned(), distribution.to_owned());
    }
    if let Some(source_date_epoch) = source_date_epoch {
        env.insert("SOURCE_DATE_EPOCH".to_owned(), format!("{}", source_date_epoch.timestamp()));
    }
    log::info!("Building debian packages, running {}.", build_command);
    match std::process::Command::new(&args[0])
        .args(&args[1..])
        .current_dir(local_tree.abspath(subpath).unwrap())
        .stdout(outf.try_clone().unwrap())
        .stderr(outf)
        .envs(env)
        .status() {
        Ok(status) => {
            if status.success() {
                log::info!("Build succeeded.");
                Ok(())
            } else {
                Err(BuildFailedError)
            }
        }
        Err(e) => {
            log::error!("Failed to run build command: {}", e);
            Err(BuildFailedError)
        }
    }
}

fn control_files_in_root(tree: &dyn Tree, subpath: &std::path::Path) -> bool {
    let debian_path = subpath.join("debian");
    if tree.has_filename(&debian_path) {
        return false;
    }
    let control_path = subpath.join("control");
    if tree.has_filename(&control_path) {
        return true;
    }
    tree.has_filename(std::path::Path::new(&format!("{}.in", control_path.to_string_lossy())))
}

/*
fn get_last_changelog_entry(
    local_tree: &WorkingTree, subpath: &std::path::Path
) -> ChangelogEntry {
    let path = if control_files_in_root(local_tree, subpath) {
        subpath.join("changelog")
    } else {
        subpath.join("debian/changelog")
    };

    let f = local_tree.get_file(&path).unwrap();

    let cl = ChangeLog::read_relaxed(f).unwrap();

    cl.entries().next().unwrap()
}
*/

pub fn gbp_dch(path: &std::path::Path) -> Result<(), std::io::Error> {
    let cmd = std::process::Command::new("gbp-dch")
        .arg("--ignore-branch")
        .current_dir(path)
        .output()?;
    if !cmd.status.success() {
        return Err(std::io::Error::new(std::io::ErrorKind::Other, "gbp-dch failed"));
    }
    Ok(())
}

pub fn find_changes_files(path: &std::path::Path, package: &str, version: &debversion::Version) -> impl Iterator<Item = (String, std::fs::DirEntry)> {
    let mut non_epoch_version = version.upstream_version.to_string();
    if let Some(debian_version) = version.debian_revision.as_ref() {
        non_epoch_version.push_str(&format!("-{}", debian_version));
    }
    let regex = format!("{}_{}_(.*)", regex::escape(package), regex::escape(&non_epoch_version));
    let c = regex::Regex::new(&regex).unwrap();

    std::fs::read_dir(path).unwrap().filter_map(move |entry| {
        let entry = entry.unwrap();
        if let Some(m) = c.captures(entry.file_name().to_str().unwrap()) {
            Some((m.get(1).unwrap().as_str().to_owned(), entry))
        } else {
            None
        }
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_build_architecture() {
        let arch = get_build_architecture();
        assert!(!arch.is_empty() && arch.len() < 10);
    }

    #[test]
    fn test_builddeb_command() {
        let command = builddeb_command(
            Some("sbuild --no-clean-source"),
            Some(std::path::Path::new("/tmp")),
            Some("ppa:my-ppa/ppa"),
            Some("my-ppa-key"),
            Some(vec!["deb http://example.com/debian buster main"]),
        );
        assert_eq!(command, vec![
            python_command(),
            "-m".to_string(),
            "breezy".to_string(),
            "builddeb".to_string(),
            "--guess-upstream-branch-url".to_string(),
            "--builder=sbuild --no-clean-source --extra-repository='deb http://example.com/debian buster main'".to_string(),
            "--apt-repository=ppa:my-ppa/ppa".to_string(),
            "--apt-repository-key=my-ppa-key".to_string(),
            "--result-dir=/tmp".to_string(),
        ]);
    }

    #[test]
    fn test_python_command() {
        let _ = python_command();
    }

    #[test]
    fn test_control_files_not_in_root() {
        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(td.path(), &breezyshim::controldir::ControlDirFormat::default()).unwrap();
        let subpath = std::path::Path::new("");

        tree.mkdir(&subpath.join("debian")).unwrap();

        tree.put_file_bytes_non_atomic(&subpath.join("debian/control"), b"").unwrap();

        assert!(!control_files_in_root(&tree, subpath));
    }

    #[test]
    fn test_control_files_in_root() {
        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(td.path(), &breezyshim::controldir::ControlDirFormat::default()).unwrap();
        let subpath = std::path::Path::new("");

        tree.put_file_bytes_non_atomic(&subpath.join("control"), b"").unwrap();

        assert!(control_files_in_root(&tree, subpath));
    }

}
