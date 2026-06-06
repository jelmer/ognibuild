use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use crate::fix_build::BuildFixer;
use crate::installer::{Error as InstallerError, Installer};
use crate::session::Session;
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
///   - cmake/meson/make: scip-clang (driven by compile_commands.json)
fn indexer_for(buildsystem: &str) -> Option<ScipIndexer> {
    match buildsystem {
        "cargo" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "rust-analyzer",
            args: &["scip", ".", "--output", OUTPUT_PLACEHOLDER],
        }),
        "setup.py" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-python",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
        }),
        "golang" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-go",
            args: &["--output", OUTPUT_PLACEHOLDER],
        }),
        "maven" | "gradle" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-java",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
        }),
        "node" => Some(ScipIndexer {
            prep: ScipPrep::None,
            binary: "scip-typescript",
            args: &["index", "--output", OUTPUT_PLACEHOLDER],
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
        }),
        _ => None,
    }
}

/// Run the preparation step required by the indexer (typically to produce a
/// `compile_commands.json`).
fn run_prep(
    prep: &ScipPrep,
    session: &dyn Session,
    installer: &dyn Installer,
) -> Result<(), Error> {
    match prep {
        ScipPrep::None => Ok(()),
        ScipPrep::CMakeCompileCommands => {
            let cmake = guaranteed_which(session, installer, "cmake")?;
            let cmake = cmake.to_str().unwrap();
            if !session.exists(Path::new("build")) {
                session.mkdir(Path::new("build"))?;
            }
            session
                .command(vec![
                    cmake,
                    ".",
                    "-Bbuild",
                    "-DCMAKE_EXPORT_COMPILE_COMMANDS=ON",
                ])
                .run_detecting_problems()?;
            session
                .command(vec![cmake, "--build", "build"])
                .run_detecting_problems()?;
            Ok(())
        }
        ScipPrep::MesonCompileCommands => {
            let meson = guaranteed_which(session, installer, "meson")?;
            let ninja = guaranteed_which(session, installer, "ninja")?;
            if !session.exists(Path::new("build")) {
                session
                    .command(vec![meson.to_str().unwrap(), "setup", "build"])
                    .run_detecting_problems()?;
            }
            session
                .command(vec![ninja.to_str().unwrap(), "-C", "build"])
                .run_detecting_problems()?;
            Ok(())
        }
        ScipPrep::BearMake => {
            let bear = guaranteed_which(session, installer, "bear")?;
            let make = guaranteed_which(session, installer, "make")?;
            session
                .command(vec![bear.to_str().unwrap(), "--", make.to_str().unwrap()])
                .run_detecting_problems()?;
            Ok(())
        }
    }
}

/// Generate a SCIP index file for the project.
///
/// Detects which build system applies and invokes the matching SCIP indexer
/// (installing it via the configured installer if it is not already on PATH).
/// For C/C++ projects, the build is run first to produce a
/// `compile_commands.json` that `scip-clang` then consumes.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of detected build systems, tried in order
/// * `installer` - Installer used to provide the indexer binary if missing
/// * `_fixers` - Reserved for future use (problem detection during indexing)
/// * `output` - Path (inside the session) where the SCIP file should be written
pub fn run_scip(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    installer: &dyn Installer,
    _fixers: &[&dyn BuildFixer<InstallerError>],
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

        run_prep(&indexer.prep, session, installer)?;

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

        session.command(argv).run_detecting_problems()?;

        log::info!("Wrote SCIP index to {}", output.display());
        return Ok(());
    }

    Err(Error::NoBuildSystemDetected)
}
