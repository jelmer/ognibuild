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

/// Run an indexer binary, resolving missing dependencies as it goes.
fn run_index_command(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    binary: &str,
    args: &[&str],
) -> Result<(), Error> {
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
}
