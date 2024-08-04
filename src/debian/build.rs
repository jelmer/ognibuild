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
}
