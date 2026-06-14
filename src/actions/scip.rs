use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use crate::fix_build::{run_fixing_problems, BuildFixer};
use crate::installer::{Error as InstallerError, Installer};
use crate::session::Session;
use std::collections::HashSet;
use std::path::Path;

/// Run the SCIP indexer for `buildsystem`, writing the index to `output`.
///
/// Each build system gets its own indexer function that does whatever that
/// indexer needs: running a build first to produce a `compile_commands.json`,
/// resolving project metadata, and so on. Returns the indexed language (used to
/// name the output file, e.g. `python`), or `None` when no indexer is known for
/// the build system. Several build systems may share a language (e.g.
/// cmake/meson/make all produce `cpp`).
fn run_indexer(
    buildsystem: &dyn BuildSystem,
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &Path,
) -> Result<Option<&'static str>, Error> {
    let output = output.to_str().ok_or_else(|| {
        Error::Other(format!(
            "Output path is not valid UTF-8: {}",
            output.display()
        ))
    })?;
    let language = match buildsystem.name() {
        "cargo" => {
            index_cargo(session, installer, fixers, output)?;
            "rust"
        }
        "setup.py" => {
            index_python(session, installer, fixers, output)?;
            "python"
        }
        "golang" => {
            index_golang(session, installer, fixers, output)?;
            "go"
        }
        "maven" | "gradle" => {
            index_java(session, installer, fixers, output)?;
            "java"
        }
        "node" => {
            index_node(session, installer, fixers, output)?;
            "typescript"
        }
        "gem" => {
            index_ruby(session, installer, fixers, output)?;
            "ruby"
        }
        "cmake" => {
            index_clang(session, installer, fixers, output, Cpp::CMake)?;
            "cpp"
        }
        "meson" => {
            index_clang(session, installer, fixers, output, Cpp::Meson)?;
            "cpp"
        }
        "make" => {
            index_clang(session, installer, fixers, output, Cpp::Make(buildsystem))?;
            "cpp"
        }
        _ => return Ok(None),
    };
    Ok(Some(language))
}

/// How a standalone release-binary indexer is packaged for download.
enum ReleaseArtifact {
    /// A bare executable; the asset is the binary itself.
    Binary,
    /// A gzipped tarball; download it and extract `member` from it.
    Tarball { member: &'static str },
}

/// A standalone release-binary indexer hosted as a GitHub release.
struct ReleaseIndexer {
    /// The session binary name this provides (and the key callers look up by).
    binary: &'static str,
    /// The GitHub `owner/repo` whose latest release is downloaded.
    repo: &'static str,
    /// The release asset to download, relative to the release. `{tag}` is
    /// replaced with the resolved release tag (e.g. `v0.12.3`) for projects
    /// that embed the version in the asset name. `{arch}` and `{goarch}` are
    /// replaced with the host architecture in the relevant naming style (e.g.
    /// `x86_64`/`aarch64` and `amd64`/`arm64`).
    asset: &'static str,
    artifact: ReleaseArtifact,
}

/// The host architecture in `x86_64`/`aarch64`-style naming (Rust's
/// `std::env::consts::ARCH`), as used by e.g. scip-clang assets.
fn host_arch() -> &'static str {
    std::env::consts::ARCH
}

/// The host architecture in Go's `GOARCH` naming (`amd64`/`arm64`), as used by
/// e.g. scip-go assets.
fn host_goarch() -> Result<&'static str, Error> {
    match std::env::consts::ARCH {
        "x86_64" => Ok("amd64"),
        "aarch64" => Ok("arm64"),
        other => Err(Error::Other(format!(
            "Unsupported architecture for release-binary indexer: {}",
            other
        ))),
    }
}

impl ReleaseIndexer {
    /// Build the download URL for the resolved release `tag`.
    fn download_url(&self, tag: &str) -> Result<String, Error> {
        let mut asset = self.asset.replace("{tag}", tag);
        if asset.contains("{arch}") {
            asset = asset.replace("{arch}", host_arch());
        }
        if asset.contains("{goarch}") {
            asset = asset.replace("{goarch}", host_goarch()?);
        }
        Ok(format!(
            "https://github.com/{}/releases/download/{}/{}",
            self.repo, tag, asset
        ))
    }
}

/// Indexers distributed only as a standalone release artifact (no apt, npm or
/// gem package). They are downloaded into the session on demand; baking them
/// into the image would not help, since an isolated session (e.g. unshare,
/// which chroots into a fresh Debian root) cannot see the host's binaries.
///
/// The release tag is resolved from the GitHub API at download time rather than
/// pinned here, so a session always gets the latest published indexer.
const RELEASE_BINARY_INDEXERS: &[ReleaseIndexer] = &[
    ReleaseIndexer {
        binary: "scip-clang",
        repo: "sourcegraph/scip-clang",
        asset: "scip-clang-{arch}-linux",
        artifact: ReleaseArtifact::Binary,
    },
    ReleaseIndexer {
        binary: "scip-go",
        repo: "sourcegraph/scip-go",
        asset: "scip-go-linux-{goarch}.tar.gz",
        artifact: ReleaseArtifact::Tarball { member: "scip-go" },
    },
    ReleaseIndexer {
        binary: "scip-java",
        repo: "sourcegraph/scip-java",
        asset: "scip-java-{tag}",
        artifact: ReleaseArtifact::Binary,
    },
];

/// Resolve the latest release tag for a GitHub `owner/repo` via the API.
///
/// A `GITHUB_TOKEN` in the environment is sent as a bearer token, raising the
/// API rate limit from 60 to 5000 requests/hour.
fn latest_release_tag(repo: &str) -> Result<String, Error> {
    let url = format!("https://api.github.com/repos/{}/releases/latest", repo);
    let mut request = reqwest::blocking::Client::new()
        .get(&url)
        .header(reqwest::header::USER_AGENT, "ognibuild")
        .header(reqwest::header::ACCEPT, "application/vnd.github+json");
    if let Ok(token) = std::env::var("GITHUB_TOKEN") {
        if !token.is_empty() {
            request = request.bearer_auth(token);
        }
    }
    let release: serde_json::Value = request
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::json)
        .map_err(|e| {
            Error::Other(format!(
                "Failed to query latest release for {}: {}",
                repo, e
            ))
        })?;
    release["tag_name"]
        .as_str()
        .map(str::to_string)
        .ok_or_else(|| Error::Other(format!("No tag_name in latest release for {}", repo)))
}

/// Fetch a URL and return its body bytes.
fn http_get_bytes(url: &str) -> Result<Vec<u8>, Error> {
    reqwest::blocking::Client::new()
        .get(url)
        .header(reqwest::header::USER_AGENT, "ognibuild")
        .send()
        .and_then(reqwest::blocking::Response::error_for_status)
        .and_then(reqwest::blocking::Response::bytes)
        .map(|b| b.to_vec())
        .map_err(|e| Error::Other(format!("Failed to download {}: {}", url, e)))
}

/// Extract a single member from a gzipped tar archive.
fn extract_tar_member(archive: &[u8], member: &str) -> Result<Vec<u8>, Error> {
    let mut tar = tar::Archive::new(flate2::read::GzDecoder::new(archive));
    let entries = tar
        .entries()
        .map_err(|e| Error::Other(format!("Failed to read tar archive: {}", e)))?;
    for entry in entries {
        let mut entry =
            entry.map_err(|e| Error::Other(format!("Failed to read tar entry: {}", e)))?;
        let path = entry
            .path()
            .map_err(|e| Error::Other(format!("Failed to read tar entry path: {}", e)))?;
        if path.as_os_str() == member {
            let mut buf = Vec::new();
            std::io::Read::read_to_end(&mut entry, &mut buf)
                .map_err(|e| Error::Other(format!("Failed to extract {}: {}", member, e)))?;
            return Ok(buf);
        }
    }
    Err(Error::Other(format!(
        "Member {} not found in tar archive",
        member
    )))
}

/// Download a standalone release-binary indexer into the session if it is not
/// already on the session's PATH. No-op for any other binary (those are
/// resolved via apt/npm/gem by the installer).
///
/// The indexer is fetched over HTTP host-side and written straight into the
/// session via `external_path`, so the session base needs no download tooling.
fn provide_release_indexer(session: &dyn Session, binary: &str) -> Result<(), Error> {
    let Some(indexer) = RELEASE_BINARY_INDEXERS
        .iter()
        .find(|indexer| indexer.binary == binary)
    else {
        return Ok(());
    };
    if crate::session::which(session, binary).is_some() {
        return Ok(());
    }
    if !session.exists(Path::new("/usr/local/bin")) {
        session.mkdir(Path::new("/usr/local/bin"))?;
    }
    // Resolve the latest release tag from the GitHub API and build the download
    // URL from it.
    let tag = latest_release_tag(indexer.repo)?;
    let url = indexer.download_url(&tag)?;
    log::info!("Downloading {} from {}", binary, url);
    let payload = http_get_bytes(&url)?;
    let bytes = match indexer.artifact {
        ReleaseArtifact::Binary => payload,
        ReleaseArtifact::Tarball { member } => extract_tar_member(&payload, member)?,
    };
    // Write the binary into the session and mark it executable. `external_path`
    // maps the session path to its host location for any session kind.
    let dest = session.external_path(&Path::new("/usr/local/bin").join(binary));
    std::fs::write(&dest, &bytes)
        .map_err(|e| Error::Other(format!("Failed to write {}: {}", dest.display(), e)))?;
    let mut perms = std::fs::metadata(&dest)?.permissions();
    std::os::unix::fs::PermissionsExt::set_mode(&mut perms, 0o755);
    std::fs::set_permissions(&dest, perms)?;
    Ok(())
}

/// Run an indexer binary, resolving missing dependencies as it goes.
fn run_index_command(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    binary: &str,
    args: &[&str],
) -> Result<(), Error> {
    provide_release_indexer(session, binary)?;
    let binary_path = guaranteed_which(session, installer, binary)?;
    let mut argv = vec![binary_path.to_str().unwrap()];
    argv.extend_from_slice(args);
    run_fixing_problems::<_, Error>(fixers, None, session, &argv, false, None, None, None)?;
    Ok(())
}

fn index_cargo(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    run_index_command(
        session,
        installer,
        fixers,
        "rust-analyzer",
        &["scip", ".", "--output", output],
    )
}

fn index_golang(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    run_index_command(session, installer, fixers, "scip-go", &["--output", output])
}

fn index_java(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    run_index_command(
        session,
        installer,
        fixers,
        "scip-java",
        &["index", "--output", output],
    )
}

fn index_node(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    run_index_command(
        session,
        installer,
        fixers,
        "scip-typescript",
        &["index", "--output", output],
    )
}

fn index_ruby(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    // scip-ruby is installed via `gem install` (the GemResolver), so the `gem`
    // command has to be present before resolving it; ensure it up front rather
    // than letting the gem resolver fail on a missing `gem`.
    guaranteed_which(session, installer, "gem")?;
    // scip-ruby reads sorbet/config when present, otherwise the trailing `.`
    // tells it to index every file in the project.
    run_index_command(
        session,
        installer,
        fixers,
        "scip-ruby",
        &["--index-file", output, "."],
    )
}

fn index_python(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
) -> Result<(), Error> {
    // scip-python crashes when it cannot determine the project name and version
    // itself (e.g. a dynamic version with no VCS metadata), so resolve them up
    // front via the project's PEP 517 wheel metadata and pass them explicitly.
    let mut args = vec!["index", "--output", output];
    let metadata = python_project_name_version(session);
    if let Some((name, version)) = metadata.as_ref() {
        args.extend(["--project-name", name, "--project-version", version]);
    } else {
        log::warn!(
            "Could not resolve Python project name/version; \
             scip-python may fail to determine them itself"
        );
    }
    run_index_command(session, installer, fixers, "scip-python", &args)
}

/// Resolve a Python project's name and version via its PEP 517 wheel metadata.
///
/// The build backend is run without build isolation (relying on the build
/// dependencies already being installed in the session), which yields the
/// resolved name and version for any backend (setuptools, poetry, flit, ...),
/// including dynamic versions. Returns None if the metadata cannot be produced
/// (e.g. python3-build is missing).
fn python_project_name_version(session: &dyn Session) -> Option<(String, String)> {
    const SNIPPET: &str = "import build.util\n\
        m = build.util.project_wheel_metadata('.', isolated=False)\n\
        print(m['Name'])\n\
        print(m['Version'])\n";
    let output = session
        .command(vec!["python3", "-c", SNIPPET])
        .quiet(true)
        .check_output()
        .ok()?;
    let text = String::from_utf8(output).ok()?;
    let mut lines = text.lines();
    let name = lines.next()?.trim().to_string();
    let version = lines.next()?.trim().to_string();
    if name.is_empty() || version.is_empty() {
        return None;
    }
    Some((name, version))
}

/// Which C/C++ build drives the `compile_commands.json` that scip-clang reads.
enum Cpp<'a> {
    CMake,
    Meson,
    /// Plain `make`, carrying the detected build system so an autotools tree can
    /// be configured before `make` runs.
    Make(&'a dyn BuildSystem),
}

fn index_clang(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &str,
    build: Cpp<'_>,
) -> Result<(), Error> {
    // scip-clang consumes a `compile_commands.json` produced by the build, so
    // run the build first to generate one.
    let compdb = match build {
        Cpp::CMake => {
            let cmake = guaranteed_which(session, installer, "cmake")?;
            let cmake = cmake.to_str().unwrap();
            if !session.exists(Path::new("build")) {
                session.mkdir(Path::new("build"))?;
            }
            run_fixing_problems::<_, Error>(
                fixers,
                None,
                session,
                &[cmake, ".", "-Bbuild", "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON"],
                false,
                None,
                None,
                None,
            )?;
            run_fixing_problems::<_, Error>(
                fixers,
                None,
                session,
                &[cmake, "--build", "build"],
                false,
                None,
                None,
                None,
            )?;
            "build/compile_commands.json"
        }
        Cpp::Meson => {
            let meson = guaranteed_which(session, installer, "meson")?;
            let ninja = guaranteed_which(session, installer, "ninja")?;
            if !session.exists(Path::new("build")) {
                run_fixing_problems::<_, Error>(
                    fixers,
                    None,
                    session,
                    &[meson.to_str().unwrap(), "setup", "build"],
                    false,
                    None,
                    None,
                    None,
                )?;
            }
            run_fixing_problems::<_, Error>(
                fixers,
                None,
                session,
                &[ninja.to_str().unwrap(), "-C", "build"],
                false,
                None,
                None,
                None,
            )?;
            "build/compile_commands.json"
        }
        Cpp::Make(buildsystem) => {
            // Autotools trees ship only configure.ac/Makefile.am and have no
            // Makefile until configured, so configure first; otherwise the bare
            // `make` below fails with "No targets specified and no makefile
            // found" and scip-clang gets no compile_commands.json.
            if let Some(make) = buildsystem
                .as_any()
                .downcast_ref::<crate::buildsystems::make::Make>()
            {
                make.configure(session, installer)?;
            }
            // Wrap make in `bear --` to intercept compiler invocations and
            // produce ./compile_commands.json.
            let bear = guaranteed_which(session, installer, "bear")?;
            let make = guaranteed_which(session, installer, "make")?;
            run_fixing_problems::<_, Error>(
                fixers,
                None,
                session,
                &[bear.to_str().unwrap(), "--", make.to_str().unwrap()],
                false,
                None,
                None,
                None,
            )?;
            "compile_commands.json"
        }
    };
    run_index_command(
        session,
        installer,
        fixers,
        "scip-clang",
        &["--compdb-path", compdb, "-o", output],
    )
}

/// Generate a SCIP index file for the project.
///
/// Detects which build system applies and invokes the matching SCIP indexer
/// (installing it via the configured installer if it is not already on PATH).
/// For C/C++ projects, the build is run first to produce a
/// `compile_commands.json` that `scip-clang` then consumes.
///
/// The first build system with a known indexer wins; remaining ones are
/// ignored. Use [`run_scip_multi`] to index every build system separately.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of detected build systems, tried in order
/// * `installer` - Installer used to provide the indexer binary if missing
/// * `fixers` - Fixers applied to resolve problems detected while running the
///   indexer and its preparation steps
/// * `output` - Path (inside the session) where the SCIP file should be written
pub fn run_scip(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &Path,
) -> Result<(), Error> {
    session.create_home()?;

    for buildsystem in buildsystems {
        let name = buildsystem.name();
        log::info!("Generating SCIP index for {}", name);
        if run_indexer(*buildsystem, session, installer, fixers, output)?.is_some() {
            log::info!("Wrote SCIP index to {}", output.display());
            return Ok(());
        }
        log::debug!("No SCIP indexer known for build system {}", name);
    }

    Err(Error::NoBuildSystemDetected)
}

/// Create a fresh temporary directory inside the session and return its path.
fn session_tempdir(session: &dyn Session) -> Result<std::path::PathBuf, Error> {
    let output = session
        .command(vec!["mktemp", "-d"])
        .quiet(true)
        .check_output()
        .map_err(|e| Error::Other(format!("Failed to create staging directory: {}", e)))?;
    let path = String::from_utf8(output)
        .map_err(|e| Error::Other(format!("mktemp output is not valid UTF-8: {}", e)))?;
    Ok(std::path::PathBuf::from(path.trim_end()))
}

/// File name (within the output directory) for a SCIP index.
///
/// Indexes are named after the language they cover (e.g. `python.scip`). When
/// two build systems in the same project map to the same language (e.g. cmake
/// and meson both produce `cpp`), `taken` already holds the plain language name,
/// so the build system is appended to disambiguate (e.g. `cpp-meson.scip`).
fn index_file_name(language: &str, buildsystem: &str, taken: &HashSet<String>) -> String {
    let plain = format!("{}.scip", language);
    if !taken.contains(&plain) {
        return plain;
    }
    format!("{}-{}.scip", language, buildsystem)
}

/// Generate one SCIP index file per detected build system.
///
/// Unlike [`run_scip`], which stops after the first build system with a known
/// indexer, this runs the indexer for every build system that has one and
/// writes the results into `output_dir`, named after the indexed language (e.g.
/// `python.scip`). When two build systems map to the same language the build
/// system is appended to disambiguate (e.g. `cpp-meson.scip`). The directory is
/// created if it does not exist.
///
/// SCIP has no native merge step, so emitting one file per build system and
/// uploading them separately is the supported way to cover a multi-language
/// project.
///
/// Indexing is best-effort: a failure for one build system does not discard the
/// indexes already written for the others. If any indexer fails the last error
/// is still returned (so the caller exits non-zero) while the successful indexes
/// remain on disk.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of detected build systems
/// * `installer` - Installer used to provide the indexer binary if missing
/// * `fixers` - Fixers applied to resolve problems detected while running the
///   indexer and its preparation steps
/// * `output_dir` - Directory (inside the session) to write the SCIP files into
pub fn run_scip_multi(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output_dir: &Path,
) -> Result<(), Error> {
    session.create_home()?;

    // `output_dir` is a host path; in an isolated session (e.g. unshare) it does
    // not exist inside the session. So index into a session-internal staging
    // directory (kept out of the project tree so the indexers do not see their
    // own output) and copy the results out onto the host afterwards via
    // `external_path`, which maps a session path to its host location for any
    // session kind (for a plain session the two coincide).
    let staging = session_tempdir(session)?;

    let mut attempted = 0;
    let mut last_error = None;
    // Indexes that succeeded, as (staging path, language, build system name).
    // The final per-language file name is resolved after the loop, once every
    // language that occurs is known.
    let mut written = Vec::new();
    for (i, buildsystem) in buildsystems.iter().enumerate() {
        let name = buildsystem.name();

        // Stage each index under a unique name so two build systems never write
        // to the same staging path; the language-based output name is resolved
        // when copying out.
        let staged = staging.join(format!("{}.scip", i));

        log::info!("Generating SCIP index for {}", name);

        // Index every build system on a best-effort basis: a failure for one
        // (e.g. a project that ships a convenience Makefile alongside its real
        // build system) must not discard the indexes that did succeed.
        match run_indexer(*buildsystem, session, installer, fixers, &staged) {
            Ok(None) => {
                log::debug!("No SCIP indexer known for build system {}", name);
            }
            Ok(Some(language)) => {
                attempted += 1;
                log::info!("Wrote SCIP index to {}", staged.display());
                written.push((staged, language, name));
            }
            Err(e) => {
                attempted += 1;
                log::warn!("Failed to generate SCIP index for {}: {}", name, e);
                last_error = Some(e);
            }
        }
    }
    let indexed = written.len();

    // Copy the indexes out of the session onto the host. `output_dir` is a host
    // path outside the session, so it is created here rather than via the
    // session; `external_path` resolves where the session wrote each file.
    if !written.is_empty() {
        std::fs::create_dir_all(output_dir)?;
        let mut taken = HashSet::new();
        for (staged, language, name) in &written {
            let file_name = index_file_name(language, name, &taken);
            taken.insert(file_name.clone());
            let src = session.external_path(staged);
            let dest = output_dir.join(&file_name);
            std::fs::copy(&src, &dest).map_err(|e| {
                Error::Other(format!(
                    "Failed to copy SCIP index {} to {}: {}",
                    src.display(),
                    dest.display(),
                    e
                ))
            })?;
        }
    }

    // Remove the staging directory; the indexes now live in `output_dir`.
    if session.exists(&staging) {
        session.rmtree(&staging)?;
    }

    if attempted == 0 {
        return Err(Error::NoBuildSystemDetected);
    }

    // Surface a failure if any indexer failed. The indexes that did succeed are
    // already written to disk, so we keep them; we just report the error so the
    // caller exits non-zero.
    if let Some(e) = last_error {
        log::warn!(
            "Generated {} of {} SCIP indexes; some build systems failed",
            indexed,
            attempted
        );
        return Err(e);
    }

    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_index_file_name() {
        let taken = HashSet::new();
        assert_eq!(index_file_name("rust", "cargo", &taken), "rust.scip");
        assert_eq!(index_file_name("python", "setup.py", &taken), "python.scip");
    }

    #[test]
    fn test_index_file_name_collision() {
        let mut taken = HashSet::new();
        assert_eq!(index_file_name("cpp", "cmake", &taken), "cpp.scip");
        taken.insert("cpp.scip".to_string());
        assert_eq!(index_file_name("cpp", "meson", &taken), "cpp-meson.scip");
    }

    #[test]
    fn test_download_url() {
        let find = |binary| {
            RELEASE_BINARY_INDEXERS
                .iter()
                .find(|i| i.binary == binary)
                .unwrap()
        };
        assert_eq!(
            find("scip-clang").download_url("v0.4.0").unwrap(),
            format!(
                "https://github.com/sourcegraph/scip-clang/releases/download/v0.4.0/scip-clang-{}-linux",
                host_arch()
            )
        );
        assert_eq!(
            find("scip-go").download_url("v0.2.7").unwrap(),
            format!(
                "https://github.com/sourcegraph/scip-go/releases/download/v0.2.7/scip-go-linux-{}.tar.gz",
                host_goarch().unwrap()
            )
        );
        // scip-java embeds the tag in the asset name.
        assert_eq!(
            find("scip-java").download_url("v0.12.3").unwrap(),
            "https://github.com/sourcegraph/scip-java/releases/download/v0.12.3/scip-java-v0.12.3"
        );
    }

    #[test]
    fn test_extract_tar_member() {
        let mut tar = tar::Builder::new(flate2::write::GzEncoder::new(
            Vec::new(),
            flate2::Compression::default(),
        ));
        let payload = b"#!/bin/sh\necho hi\n";
        let mut header = tar::Header::new_gnu();
        header.set_size(payload.len() as u64);
        header.set_mode(0o755);
        header.set_cksum();
        tar.append_data(&mut header, "scip-go", &payload[..])
            .unwrap();
        let gz = tar.into_inner().unwrap().finish().unwrap();

        assert_eq!(extract_tar_member(&gz, "scip-go").unwrap(), payload);
        assert!(extract_tar_member(&gz, "missing").is_err());
    }
}
