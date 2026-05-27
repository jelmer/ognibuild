use crate::buildsystem::{guaranteed_which, BuildSystem, Error};
use crate::fix_build::BuildFixer;
use crate::installer::{Error as InstallerError, Installer};
use crate::session::Session;
use std::path::Path;

/// A SCIP indexer that can be invoked for a particular build system.
struct ScipIndexer {
    /// The binary that produces the SCIP index.
    binary: &'static str,
    /// Build arguments after the binary; "%OUTPUT%" is substituted with the
    /// resolved output path inside the session.
    args: &'static [&'static str],
}

/// Look up the SCIP indexer that should be used for a given build system name.
///
/// Returns None when no indexer is known for this build system. The mapping is
/// based on the public SCIP indexers maintained by Sourcegraph:
///   - cargo:        scip-rust (rust-analyzer scip)
///   - setup.py:     scip-python
///   - go:           scip-go
///   - maven/gradle: scip-java
///   - node:         scip-typescript
fn indexer_for(buildsystem: &str) -> Option<ScipIndexer> {
    match buildsystem {
        "cargo" => Some(ScipIndexer {
            binary: "rust-analyzer",
            args: &["scip", ".", "--output", "%OUTPUT%"],
        }),
        "setup.py" => Some(ScipIndexer {
            binary: "scip-python",
            args: &["index", "--output", "%OUTPUT%"],
        }),
        "go" => Some(ScipIndexer {
            binary: "scip-go",
            args: &["--output", "%OUTPUT%"],
        }),
        "maven" | "gradle" => Some(ScipIndexer {
            binary: "scip-java",
            args: &["index", "--output", "%OUTPUT%"],
        }),
        "node" => Some(ScipIndexer {
            binary: "scip-typescript",
            args: &["index", "--output", "%OUTPUT%"],
        }),
        _ => None,
    }
}

/// Generate a SCIP index file for the project.
///
/// Detects which build system applies and invokes the matching SCIP indexer
/// (installing it via the configured installer if it is not already on PATH).
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
            argv.push(if *arg == "%OUTPUT%" { output_str } else { *arg });
        }

        session.command(argv).run_detecting_problems()?;

        log::info!("Wrote SCIP index to {}", output.display());
        return Ok(());
    }

    Err(Error::NoBuildSystemDetected)
}
