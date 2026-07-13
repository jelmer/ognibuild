use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use crate::fix_build::BuildFixer;
use crate::installer::{Error as InstallerError, Installer};
use crate::session::Session;
use std::path::Path;

/// Substituted with the (UTF-8) output path in indexer arguments.
const OUTPUT_PLACEHOLDER: &str = "%OUTPUT%";

/// A preparation step run before invoking the LSIF indexer.
///
/// Most language indexers (cargo, go, ...) read source directly and need no
/// preparation. C/C++ indexing via `lsif-clang` is different: it consumes a
/// `compile_commands.json` produced by the build, so the build must run first.
enum LsifPrep {
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

/// How the indexer emits its LSIF dump.
enum Output {
    /// The indexer writes the dump to the path given via `OUTPUT_PLACEHOLDER`
    /// in its arguments (lsif-go, lsif-tsc, lsif-clang).
    File,
    /// The indexer writes the dump to stdout, which we redirect to the output
    /// file ourselves (rust-analyzer lsif).
    Stdout,
}

/// An LSIF indexer that can be invoked for a particular build system.
struct LsifIndexer {
    /// Optional preparation step (e.g. running the build to produce
    /// `compile_commands.json`).
    prep: LsifPrep,
    /// The binary that produces the LSIF index.
    binary: &'static str,
    /// Arguments to the indexer binary. Any occurrence of `OUTPUT_PLACEHOLDER`
    /// is substituted with the resolved output path.
    args: &'static [&'static str],
    /// How the indexer emits its dump.
    output: Output,
}

/// Look up the LSIF indexer that should be used for a given build system name.
///
/// Returns None when no indexer is known for this build system. The mapping is
/// based on the public LSIF indexers maintained by Sourcegraph:
///   - cargo:        rust-analyzer lsif (writes to stdout)
///   - go:           lsif-go
///   - node:         lsif-tsc
///   - cmake/meson/make: lsif-clang (driven by compile_commands.json)
fn indexer_for(buildsystem: &str) -> Option<LsifIndexer> {
    match buildsystem {
        "cargo" => Some(LsifIndexer {
            prep: LsifPrep::None,
            binary: "rust-analyzer",
            args: &["lsif", "."],
            output: Output::Stdout,
        }),
        "go" => Some(LsifIndexer {
            prep: LsifPrep::None,
            binary: "lsif-go",
            args: &["--output", OUTPUT_PLACEHOLDER],
            output: Output::File,
        }),
        "node" => Some(LsifIndexer {
            prep: LsifPrep::None,
            binary: "lsif-tsc",
            args: &["-p", ".", "--out", OUTPUT_PLACEHOLDER],
            output: Output::File,
        }),
        "cmake" => Some(LsifIndexer {
            prep: LsifPrep::CMakeCompileCommands,
            binary: "lsif-clang",
            args: &["build/compile_commands.json", "-o", OUTPUT_PLACEHOLDER],
            output: Output::File,
        }),
        "meson" => Some(LsifIndexer {
            prep: LsifPrep::MesonCompileCommands,
            binary: "lsif-clang",
            args: &["build/compile_commands.json", "-o", OUTPUT_PLACEHOLDER],
            output: Output::File,
        }),
        "make" => Some(LsifIndexer {
            prep: LsifPrep::BearMake,
            binary: "lsif-clang",
            args: &["compile_commands.json", "-o", OUTPUT_PLACEHOLDER],
            output: Output::File,
        }),
        _ => None,
    }
}

/// Run the preparation step required by the indexer (typically to produce a
/// `compile_commands.json`).
fn run_prep(
    prep: &LsifPrep,
    session: &dyn Session,
    installer: &dyn Installer,
) -> Result<(), Error> {
    match prep {
        LsifPrep::None => Ok(()),
        LsifPrep::CMakeCompileCommands => {
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
        LsifPrep::MesonCompileCommands => {
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
        LsifPrep::BearMake => {
            let bear = guaranteed_which(session, installer, "bear")?;
            let make = guaranteed_which(session, installer, "make")?;
            session
                .command(vec![bear.to_str().unwrap(), "--", make.to_str().unwrap()])
                .run_detecting_problems()?;
            Ok(())
        }
    }
}

/// Generate an LSIF index file for the project.
///
/// Detects which build system applies and invokes the matching LSIF indexer
/// (installing it via the configured installer if it is not already on PATH).
/// For C/C++ projects, the build is run first to produce a
/// `compile_commands.json` that `lsif-clang` then consumes.
///
/// # Arguments
/// * `session` - The session to run commands in
/// * `buildsystems` - List of detected build systems, tried in order
/// * `installer` - Installer used to provide the indexer binary if missing
/// * `_fixers` - Reserved for future use (problem detection during indexing)
/// * `output` - Path (inside the session) where the LSIF file should be written
pub fn run_lsif(
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
            log::debug!("No LSIF indexer known for build system {}", name);
            continue;
        };

        log::info!(
            "Generating LSIF index for {} using {}",
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

        match indexer.output {
            Output::File => {
                session.command(argv).run_detecting_problems()?;
            }
            Output::Stdout => {
                // The indexer streams the dump to stdout; redirect that straight
                // into the output file.
                let file = std::fs::File::create(session.external_path(output))?;
                let status = session
                    .command(argv)
                    .stdout(std::process::Stdio::from(file))
                    .run()?;
                if !status.success() {
                    return Err(Error::Other(format!(
                        "{} exited with status {}",
                        indexer.binary, status
                    )));
                }
            }
        }

        log::info!("Wrote LSIF index to {}", output.display());
        return Ok(());
    }

    Err(Error::NoBuildSystemDetected)
}
