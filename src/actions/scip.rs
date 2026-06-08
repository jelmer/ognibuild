use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use crate::fix_build::{run_fixing_problems, BuildFixer};
use crate::installer::{Error as InstallerError, Installer};
use crate::session::Session;
use std::collections::HashSet;
use std::path::Path;

/// Substituted with the (UTF-8) output path in indexer arguments.
const OUTPUT_PLACEHOLDER: &str = "%OUTPUT%";

/// A preparation step run before invoking the SCIP indexer.
///
/// Most language indexers (cargo, go, ...) read source directly and need no
/// preparation. C/C++ indexing via `scip-clang` is different: it consumes a
/// `compile_commands.json` produced by the build, so the build must run first.
enum ScipPrep {
    /// No preparation step; the indexer reads sources directly.
    None,
    /// Configure with CMake (`-DCMAKE_EXPORT_COMPILE_COMMANDS=ON`) and build,
    /// producing `build/compile_commands.json`.
    CMakeCompileCommands,
    /// Configure with Meson and build with ninja; meson always emits
    /// `build/compile_commands.json`.
    MesonCompileCommands,
    /// Wrap `make` in `bear --` to intercept compiler invocations and produce
    /// `./compile_commands.json`.
    BearMake,
}

/// A SCIP indexer that can be invoked for a particular build system.
struct ScipIndexer {
    /// Optional preparation step (e.g. running the build to produce
    /// `compile_commands.json`).
    prep: ScipPrep,
    /// The binary that produces the SCIP index.
    binary: &'static str,
    /// Arguments to the indexer binary. Any occurrence of `OUTPUT_PLACEHOLDER`
    /// is substituted with the resolved output path.
    args: &'static [&'static str],
    /// Language indexed, used to name the output file (e.g. `python`).
    /// Several build systems may share a language (e.g. cmake/meson/make all
    /// produce `cpp`).
    language: &'static str,
}

/// Look up the SCIP indexer that should be used for a given build system name.
///
/// Returns None when no indexer is known for this build system. The mapping is
/// based on the public SCIP indexers maintained by Sourcegraph:
///   - cargo:        rust-analyzer scip
///   - setup.py:     scip-python
///   - golang:       scip-go
///   - maven/gradle: scip-java
///   - node:         scip-typescript
///   - gem:          scip-ruby
///   - cmake/meson/make: scip-clang (driven by compile_commands.json)
fn indexer_for(buildsystem: &str) -> Option<ScipIndexer> {
    match buildsystem {
        "cargo" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "rust-analyzer",
            args: &["scip", ".", "--output", OUTPUT_PLACEHOLDER],
            language: "rust",
        }),
        "setup.py" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-python",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
            language: "python",
        }),
        "golang" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-go",
            args: &["--output", OUTPUT_PLACEHOLDER],
            language: "go",
        }),
        "maven" | "gradle" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-java",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
            language: "java",
        }),
        "node" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-typescript",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
            language: "typescript",
        }),
        // scip-ruby reads sorbet/config when present, otherwise the trailing `.`
        // tells it to index every file in the project.
        "gem" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-ruby",
            args: &["--index-file", OUTPUT_PLACEHOLDER, "."],
            language: "ruby",
        }),
        "cmake" => Some(ScipIndexer {
            prep: ScipPrep::CMakeCompileCommands,
            binary: "scip-clang",
            args: &[
                "--compdb-path",
                "build/compile_commands.json",
                "-o",
                OUTPUT_PLACEHOLDER,
            ],
            language: "cpp",
        }),
        "meson" => Some(ScipIndexer {
            prep: ScipPrep::MesonCompileCommands,
            binary: "scip-clang",
            args: &[
                "--compdb-path",
                "build/compile_commands.json",
                "-o",
                OUTPUT_PLACEHOLDER,
            ],
            language: "cpp",
        }),
        "make" => Some(ScipIndexer {
            prep: ScipPrep::BearMake,
            binary: "scip-clang",
            args: &[
                "--compdb-path",
                "compile_commands.json",
                "-o",
                OUTPUT_PLACEHOLDER,
            ],
            language: "cpp",
        }),
        _ => None,
    }
}

/// Run the preparation step required by the indexer (typically to produce a
/// `compile_commands.json`).
///
/// The prep commands shell out to the project's own build tooling, which can
/// hit missing dependencies the same way an ordinary build does, so they run
/// through `run_fixing_problems` to resolve and retry rather than aborting.
fn run_prep(
    prep: &ScipPrep,
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
) -> Result<(), Error> {
    match prep {
        ScipPrep::None => Ok(()),
        ScipPrep::CMakeCompileCommands => {
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
            Ok(())
        }
        ScipPrep::MesonCompileCommands => {
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
            Ok(())
        }
        ScipPrep::BearMake => {
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
            Ok(())
        }
    }
}

/// Run a single indexer to produce a SCIP index at `output`.
fn run_indexer(
    indexer: &ScipIndexer,
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    output: &Path,
) -> Result<(), Error> {
    run_prep(&indexer.prep, session, installer, fixers)?;

    let binary_path = guaranteed_which(session, installer, indexer.binary)?;

    let output_str = output.to_str().ok_or_else(|| {
        Error::Other(format!(
            "Output path is not valid UTF-8: {}",
            output.display()
        ))
    })?;

    let mut argv: Vec<&str> = Vec::with_capacity(indexer.args.len() + 1);
    argv.push(binary_path.to_str().unwrap());
    for arg in indexer.args {
        argv.push(if *arg == OUTPUT_PLACEHOLDER {
            output_str
        } else {
            *arg
        });
    }

    run_fixing_problems::<_, Error>(fixers, None, session, &argv, false, None, None, None)?;
    Ok(())
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
        let Some(indexer) = indexer_for(name) else {
            log::debug!("No SCIP indexer known for build system {}", name);
            continue;
        };

        log::info!(
            "Generating SCIP index for {} using {}",
            name,
            indexer.binary
        );

        run_indexer(&indexer, session, installer, fixers, output)?;

        log::info!("Wrote SCIP index to {}", output.display());
        return Ok(());
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

    let mut indexed = 0;
    let mut attempted = 0;
    let mut last_error = None;
    let mut taken = HashSet::new();
    let mut written = Vec::new();
    for buildsystem in buildsystems {
        let name = buildsystem.name();
        let Some(indexer) = indexer_for(name) else {
            log::debug!("No SCIP indexer known for build system {}", name);
            continue;
        };

        let file_name = index_file_name(indexer.language, name, &taken);
        taken.insert(file_name.clone());
        let output = staging.join(&file_name);

        log::info!(
            "Generating SCIP index for {} using {}",
            name,
            indexer.binary
        );

        // Index every build system on a best-effort basis: a failure for one
        // (e.g. a project that ships a convenience Makefile alongside its real
        // build system) must not discard the indexes that did succeed.
        attempted += 1;
        match run_indexer(&indexer, session, installer, fixers, &output) {
            Ok(()) => {
                log::info!("Wrote SCIP index to {}", output.display());
                written.push(file_name);
                indexed += 1;
            }
            Err(e) => {
                log::warn!(
                    "Failed to generate SCIP index for {} using {}: {}",
                    name,
                    indexer.binary,
                    e
                );
                last_error = Some(e);
            }
        }
    }

    // Copy the indexes out of the session onto the host. `output_dir` is a host
    // path outside the session, so it is created here rather than via the
    // session; `external_path` resolves where the session wrote each file.
    if !written.is_empty() {
        std::fs::create_dir_all(output_dir)?;
        for file_name in &written {
            let src = session.external_path(&staging.join(file_name));
            let dest = output_dir.join(file_name);
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
    fn test_indexer_known_and_unknown() {
        assert!(indexer_for("cargo").is_some());
        assert!(indexer_for("unknown").is_none());
    }

    #[test]
    fn test_indexer_golang_matches_build_system_name() {
        // The Go build system reports its name as "golang", not "go".
        assert_eq!(indexer_for("golang").map(|i| i.binary), Some("scip-go"));
        assert!(indexer_for("go").is_none());
    }

    #[test]
    fn test_indexer_gem_matches_build_system_name() {
        // The Ruby build system reports its name as "gem".
        assert_eq!(indexer_for("gem").map(|i| i.binary), Some("scip-ruby"));
        assert_eq!(indexer_for("gem").map(|i| i.language), Some("ruby"));
    }
}
