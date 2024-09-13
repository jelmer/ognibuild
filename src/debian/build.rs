use breezyshim::tree::{MutableTree, Tree, WorkingTree};
use buildlog_consultant::Problem;
use debian_changelog::{ChangeLog, Urgency};
use debversion::Version;
use std::io::Seek;
use std::path::{Path, PathBuf};

pub fn get_build_architecture() -> String {
    std::process::Command::new("dpkg-architecture")
        .arg("-qDEB_BUILD_ARCH")
        .output()
        .map(|output| String::from_utf8(output.stdout).unwrap().trim().to_string())
        .unwrap()
}

pub const DEFAULT_BUILDER: &str = "sbuild --no-clean-source";

fn python_command() -> String {
    pyo3::Python::with_gil(|py| {
        use pyo3::types::PyAnyMethods;
        let sys_module = py.import_bound("sys").unwrap();
        sys_module
            .getattr("executable")
            .unwrap()
            .extract::<String>()
            .unwrap()
    })
}

pub fn builddeb_command(
    build_command: Option<&str>,
    result_dir: Option<&std::path::Path>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<&Vec<&str>>,
) -> Vec<String> {
    let mut build_command = build_command.unwrap_or(DEFAULT_BUILDER).to_string();

    if let Some(extra_repositories) = extra_repositories {
        for repo in extra_repositories {
            build_command.push_str(&format!(
                " --extra-repository={}",
                shlex::try_quote(repo).unwrap()
            ));
        }
    }

    let mut args = vec![
        python_command(),
        "-m".to_string(),
        "breezy".to_string(),
        "builddeb".to_string(),
        "--guess-upstream-branch-url".to_string(),
        format!("--builder={}", build_command),
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
pub struct BuildFailedError(pub i32);

impl std::fmt::Display for BuildFailedError {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        write!(f, "Build failed: {}", self.0)
    }
}

impl std::error::Error for BuildFailedError {}

pub fn build(
    local_tree: &WorkingTree,
    outf: std::fs::File,
    build_command: &str,
    result_dir: Option<&std::path::Path>,
    distribution: Option<&str>,
    subpath: &std::path::Path,
    source_date_epoch: Option<chrono::DateTime<chrono::Utc>>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<&Vec<&str>>,
) -> Result<(), BuildFailedError> {
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
        env.insert(
            "SOURCE_DATE_EPOCH".to_owned(),
            format!("{}", source_date_epoch.timestamp()),
        );
    }
    log::info!("Building debian packages, running {}.", build_command);
    match std::process::Command::new(&args[0])
        .args(&args[1..])
        .current_dir(local_tree.abspath(subpath).unwrap())
        .stdout(outf.try_clone().unwrap())
        .stderr(outf)
        .envs(env)
        .status()
    {
        Ok(status) => {
            if status.success() {
                log::info!("Build succeeded.");
                Ok(())
            } else {
                Err(BuildFailedError(status.code().unwrap_or(1)))
            }
        }
        Err(e) => {
            log::error!("Failed to run build command: {}", e);
            Err(BuildFailedError(1))
        }
    }
}

pub const BUILD_LOG_FILENAME: &str = "build.log";

#[derive(Debug)]
pub enum BuildOnceError {
    Detailed {
        stage: Option<String>,
        phase: Option<buildlog_consultant::sbuild::Phase>,
        retcode: i32,
        command: Vec<String>,
        error: Box<dyn Problem>,
        description: String,
    },
    Unidentified {
        stage: Option<String>,
        phase: Option<buildlog_consultant::sbuild::Phase>,
        retcode: i32,
        command: Vec<String>,
        description: String,
    },
}

pub struct BuildOnceResult {
    pub source_package: String,
    pub version: debversion::Version,
    pub changes_names: Vec<PathBuf>,
}

pub fn build_once(
    local_tree: &WorkingTree,
    build_suite: Option<&str>,
    output_directory: &Path,
    build_command: &str,
    subpath: &Path,
    source_date_epoch: Option<chrono::DateTime<chrono::Utc>>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<&Vec<&str>>,
) -> Result<BuildOnceResult, BuildOnceError> {
    use buildlog_consultant::problems::debian::DpkgSourceLocalChanges;
    use buildlog_consultant::sbuild::{worker_failure_from_sbuild_log, SbuildLog};
    let build_log_path = output_directory.join(BUILD_LOG_FILENAME);
    log::debug!("Writing build log to {}", build_log_path.display());
    let mut logf = std::fs::File::create(&build_log_path).unwrap();
    match build(
        local_tree,
        logf.try_clone().unwrap(),
        build_command,
        Some(output_directory),
        build_suite,
        subpath,
        source_date_epoch,
        apt_repository,
        apt_repository_key,
        extra_repositories,
    ) {
        Ok(()) => (),
        Err(e) => {
            logf.sync_all().unwrap();
            logf.seek(std::io::SeekFrom::Start(0)).unwrap();

            let sbuildlog =
                SbuildLog::try_from(std::fs::File::open(&build_log_path).unwrap()).unwrap();
            let sbuild_failure = worker_failure_from_sbuild_log(&sbuildlog);

            // Preserve the diff for later inspection
            if let Some(error) = sbuild_failure
                .error
                .as_ref()
                .and_then(|e| e.as_any().downcast_ref::<DpkgSourceLocalChanges>())
            {
                if let Some(diff_file) = error.diff_file.as_ref() {
                    let diff_file_name =
                        output_directory.join(Path::new(&diff_file).file_name().unwrap());
                    std::fs::copy(diff_file, &diff_file_name).unwrap();
                }
            }

            let retcode = e.0;
            if let Some(error) = sbuild_failure.error {
                return Err(BuildOnceError::Detailed {
                    stage: sbuild_failure.stage,
                    phase: sbuild_failure.phase,
                    retcode,
                    command: shlex::split(build_command).unwrap(),
                    error,
                    description: sbuild_failure.description.unwrap_or_default(),
                });
            } else {
                return Err(BuildOnceError::Unidentified {
                    stage: sbuild_failure.stage,
                    phase: sbuild_failure.phase,
                    retcode,
                    command: shlex::split(build_command).unwrap(),
                    description: sbuild_failure
                        .description
                        .unwrap_or_else(|| format!("Build failed with exit code {}", retcode)),
                });
            }
        }
    }

    let (package, version) = get_last_changelog_entry(local_tree, subpath);
    let mut changes_names = vec![];
    for (_kind, entry) in find_changes_files(output_directory, &package, &version) {
        changes_names.push(entry.path());
    }
    Ok(BuildOnceResult {
        source_package: package,
        version,
        changes_names,
    })
}

fn control_files_in_root(tree: &dyn MutableTree, subpath: &std::path::Path) -> bool {
    let debian_path = subpath.join("debian");
    if tree.has_filename(&debian_path) {
        return false;
    }
    let control_path = subpath.join("control");
    if tree.has_filename(&control_path) {
        return true;
    }
    tree.has_filename(std::path::Path::new(&format!(
        "{}.in",
        control_path.to_string_lossy()
    )))
}

fn get_last_changelog_entry(
    local_tree: &WorkingTree,
    subpath: &std::path::Path,
) -> (String, debversion::Version) {
    let path = if control_files_in_root(local_tree, subpath) {
        subpath.join("changelog")
    } else {
        subpath.join("debian/changelog")
    };

    let f = local_tree.get_file(&path).unwrap();

    let cl = ChangeLog::read_relaxed(f).unwrap();

    let e = cl.entries().next().unwrap();

    (e.package().unwrap(), e.version().unwrap())
}

pub fn gbp_dch(path: &std::path::Path) -> Result<(), std::io::Error> {
    let cmd = std::process::Command::new("gbp-dch")
        .arg("--ignore-branch")
        .current_dir(path)
        .output()?;
    if !cmd.status.success() {
        return Err(std::io::Error::new(
            std::io::ErrorKind::Other,
            "gbp-dch failed",
        ));
    }
    Ok(())
}

pub fn find_changes_files(
    path: &std::path::Path,
    package: &str,
    version: &debversion::Version,
) -> impl Iterator<Item = (String, std::fs::DirEntry)> {
    let mut non_epoch_version = version.upstream_version.to_string();
    if let Some(debian_version) = version.debian_revision.as_ref() {
        non_epoch_version.push_str(&format!("-{}", debian_version));
    }
    let regex = format!(
        "{}_{}_(.*)",
        regex::escape(package),
        regex::escape(&non_epoch_version)
    );
    let c = regex::Regex::new(&regex).unwrap();

    std::fs::read_dir(path).unwrap().filter_map(move |entry| {
        let entry = entry.unwrap();
        c.captures(entry.file_name().to_str().unwrap())
            .map(|m| (m.get(1).unwrap().as_str().to_owned(), entry))
    })
}

/// Attempt a build, with a custom distribution set.
///
/// # Arguments
/// * `local_tree` - The tree to build in.
/// * `suffix` - Suffix to add to version string.
/// * `build_suite` - Name of suite (i.e. distribution) to build for.
/// * `output_directory` - Directory to write output to.
/// * `build_command` - Build command to build package.
/// * `build_changelog_entry` - Changelog entry to use.
/// * `subpath` - Sub path in tree where package lives.
/// * `source_date_epoch` - Source date epoch to set.
/// * `run_gbp_dch` - Whether to run gbp-dch.
/// * `apt_repository` - APT repository to use.
/// * `apt_repository_key` - APT repository key to use.
/// * `extra_repositories` - Extra repositories to use.
pub fn attempt_build(
    local_tree: &WorkingTree,
    suffix: Option<&str>,
    build_suite: Option<&str>,
    output_directory: &std::path::Path,
    build_command: &str,
    build_changelog_entry: Option<&str>,
    subpath: &std::path::Path,
    source_date_epoch: Option<chrono::DateTime<chrono::Utc>>,
    run_gbp_dch: bool,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<&Vec<&str>>,
) -> Result<BuildOnceResult, BuildOnceError> {
    if run_gbp_dch
        && subpath == std::path::Path::new("")
        && pyo3::Python::with_gil(|py| {
            use pyo3::ToPyObject;
            local_tree
                .controldir()
                .to_object(py)
                .getattr(py, "_git")
                .is_ok()
        })
    {
        gbp_dch(&local_tree.abspath(subpath).unwrap()).unwrap();
    }
    if let Some(build_changelog_entry) = build_changelog_entry {
        assert!(
            suffix.is_some(),
            "build_changelog_entry specified, but suffix is None"
        );
        assert!(
            build_suite.is_some(),
            "build_changelog_entry specified, but build_suite is None"
        );
        add_dummy_changelog_entry(
            local_tree,
            subpath,
            suffix.unwrap(),
            build_suite.unwrap(),
            build_changelog_entry,
            None,
            None,
        );
    }
    build_once(
        local_tree,
        build_suite,
        output_directory,
        build_command,
        subpath,
        source_date_epoch,
        apt_repository,
        apt_repository_key,
        extra_repositories,
    )
}

pub fn version_add_suffix(version: &Version, suffix: &str) -> Version {
    fn add_suffix(v: &str, suffix: &str) -> String {
        if let Some(m) = regex::Regex::new(&format!("(.*)({})([0-9]+)", regex::escape(suffix)))
            .unwrap()
            .captures(v)
        {
            let main = m.get(1).unwrap().as_str();
            let suffix = m.get(2).unwrap().as_str();
            let revision = m.get(3).unwrap().as_str();
            format!("{}{}{}", main, suffix, revision.parse::<u64>().unwrap() + 1)
        } else {
            format!("{}{}1", v, suffix)
        }
    }

    let mut version = version.clone();
    if let Some(r) = version.debian_revision {
        version.debian_revision = Some(add_suffix(&r, suffix));
    } else {
        version.upstream_version = add_suffix(&version.upstream_version, suffix);
    }
    version
}

/// Add a dummy changelog entry to a package.
///
/// # Arguments
/// * `tree` - The tree to add the entry to.
/// * `subpath` - Sub path in tree where package lives.
/// * `suffix` - Suffix to add to version string.
/// * `suite` - Name of suite (i.e. distribution) to build for.
/// * `message` - Changelog message to use.
/// * `timestamp` - Timestamp to use.
/// * `maintainer` - Maintainer to use.
/// * `allow_reformatting` - Whether to allow reformatting.
///
/// # Returns
/// The version of the newly added entry.
pub fn add_dummy_changelog_entry(
    tree: &dyn MutableTree,
    subpath: &Path,
    suffix: &str,
    suite: &str,
    message: &str,
    timestamp: Option<chrono::DateTime<chrono::Utc>>,
    maintainer: Option<(String, String)>,
) -> Version {
    let path = if control_files_in_root(tree, subpath) {
        subpath.join("changelog")
    } else {
        subpath.join("debian/changelog")
    };
    let mut cl = ChangeLog::read_relaxed(tree.get_file(&path).unwrap()).unwrap();

    let prev_entry = cl.entries().next().unwrap();
    let prev_version = prev_entry.version().unwrap();

    let version = version_add_suffix(&prev_version, suffix);
    log::debug!("Adding dummy changelog entry {} for build", &version);
    let mut entry = cl.auto_add_change(
        &[&format!("* {}", message)],
        maintainer.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap()),
        timestamp.map(|t| t.into()),
        Some(Urgency::Low),
    );
    entry.set_version(&version);
    entry.set_distributions(vec![suite.to_string()]);

    tree.put_file_bytes_non_atomic(&path, cl.to_string().as_bytes())
        .unwrap();

    entry.version().unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::tree::MutableTree;

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
            Some(&vec!["deb http://example.com/debian buster main"]),
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
        let tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        let subpath = std::path::Path::new("");

        tree.mkdir(&subpath.join("debian")).unwrap();

        tree.put_file_bytes_non_atomic(&subpath.join("debian/control"), b"")
            .unwrap();

        assert!(!control_files_in_root(&tree, subpath));
    }

    #[test]
    fn test_control_files_in_root() {
        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(
            td.path(),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        let subpath = std::path::Path::new("");

        tree.put_file_bytes_non_atomic(&subpath.join("control"), b"")
            .unwrap();

        assert!(control_files_in_root(&tree, subpath));
    }

    mod test_version_add_suffix {
        use super::*;

        #[test]
        fn test_native() {
            assert_eq!(
                "1.0~jan+lint4".parse::<Version>().unwrap(),
                version_add_suffix(&"1.0~jan+lint3".parse().unwrap(), "~jan+lint"),
            );
            assert_eq!(
                "1.0~jan+lint1".parse::<Version>().unwrap(),
                version_add_suffix(&"1.0".parse().unwrap(), "~jan+lint"),
            );
        }

        #[test]
        fn test_normal() {
            assert_eq!(
                "1.0-1~jan+lint4".parse::<Version>().unwrap(),
                version_add_suffix(&"1.0-1~jan+lint3".parse().unwrap(), "~jan+lint"),
            );
            assert_eq!(
                "1.0-1~jan+lint1".parse::<Version>().unwrap(),
                version_add_suffix(&"1.0-1".parse().unwrap(), "~jan+lint"),
            );
            assert_eq!(
                "0.0.12-1~jan+lint1".parse::<Version>().unwrap(),
                version_add_suffix(&"0.0.12-1".parse().unwrap(), "~jan+lint"),
            );
            assert_eq!(
                "0.0.12-1~jan+unchanged1~jan+lint1"
                    .parse::<Version>()
                    .unwrap(),
                version_add_suffix(&"0.0.12-1~jan+unchanged1".parse().unwrap(), "~jan+lint"),
            );
        }
    }

    mod test_add_dummy_changelog {
        use super::*;
        use std::path::Path;
        #[test]
        fn test_simple() {
            let td = tempfile::tempdir().unwrap();
            let tree = breezyshim::controldir::create_standalone_workingtree(
                td.path(),
                &breezyshim::controldir::ControlDirFormat::default(),
            )
            .unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r#"janitor (0.1-1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
            )
            .unwrap();
            tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
                .unwrap();
            add_dummy_changelog_entry(
                &tree,
                Path::new(""),
                "jan+some",
                "some-fixes",
                "Dummy build.",
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-05T12:35:04Z")
                        .unwrap()
                        .to_utc(),
                ),
                Some(("Jelmer Vernooĳ".to_owned(), "jelmer@debian.org".to_owned())),
            );

            let contents = std::fs::read_to_string(td.path().join("debian/changelog")).unwrap();
            assert_eq!(
                r#"janitor (0.1-1jan+some1) some-fixes; urgency=medium

  * Initial release. (Closes: #XXXXXX)
  * Dummy build.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
                contents
            );
        }

        #[test]
        fn test_native() {
            let td = tempfile::tempdir().unwrap();
            let tree = breezyshim::controldir::create_standalone_workingtree(
                td.path(),
                &breezyshim::controldir::ControlDirFormat::default(),
            )
            .unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r#"janitor (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
            )
            .unwrap();
            tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
                .unwrap();
            add_dummy_changelog_entry(
                &tree,
                Path::new(""),
                "jan+some",
                "some-fixes",
                "Dummy build.",
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-05T12:35:04Z")
                        .unwrap()
                        .to_utc(),
                ),
                Some(("Jelmer Vernooĳ".to_owned(), "jelmer@debian.org".to_owned())),
            );

            let contents = std::fs::read_to_string(td.path().join("debian/changelog")).unwrap();
            assert_eq!(
                r#"janitor (0.1jan+some1) some-fixes; urgency=medium

  * Initial release. (Closes: #XXXXXX)
  * Dummy build.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
                contents
            );
        }

        #[test]
        fn test_exists() {
            let td = tempfile::tempdir().unwrap();
            let tree = breezyshim::controldir::create_standalone_workingtree(
                td.path(),
                &breezyshim::controldir::ControlDirFormat::default(),
            )
            .unwrap();
            std::fs::create_dir(td.path().join("debian")).unwrap();
            std::fs::write(
                td.path().join("debian/changelog"),
                r#"janitor (0.1-1jan+some1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
            )
            .unwrap();
            tree.add(&[Path::new("debian"), Path::new("debian/changelog")])
                .unwrap();
            add_dummy_changelog_entry(
                &tree,
                Path::new(""),
                "jan+some",
                "some-fixes",
                "Dummy build.",
                Some(
                    chrono::DateTime::parse_from_rfc3339("2020-09-05T12:35:04Z")
                        .unwrap()
                        .to_utc(),
                ),
                Some(("Jelmer Vernooĳ".to_owned(), "jelmer@debian.org".to_owned())),
            );
            let contents = std::fs::read_to_string(td.path().join("debian/changelog")).unwrap();
            assert_eq!(
                r#"janitor (0.1-1jan+some2) some-fixes; urgency=medium

  * Initial release. (Closes: #XXXXXX)
  * Dummy build.

 -- Jelmer Vernooĳ <jelmer@debian.org>  Sat, 05 Sep 2020 12:35:04 -0000
"#,
                contents
            );
        }
    }
}
