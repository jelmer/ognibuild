use crate::buildsystem::{BuildSystem, Error};
use crate::fix_build::BuildFixer;
use crate::installer::Error as InstallerError;
use crate::session::Session;
use std::collections::HashMap;

pub fn run_info(
    session: &dyn Session,
    buildsystems: &[&dyn BuildSystem],
    fixers: Option<&[&dyn BuildFixer<InstallerError>]>,
) -> Result<(), Error> {
    for buildsystem in buildsystems {
        log::info!("{:?}", buildsystem);
        let mut deps = HashMap::new();
        match buildsystem.get_declared_dependencies(session, fixers) {
            Ok(declared_deps) => {
                for (category, dep) in declared_deps {
                    deps.entry(category).or_insert_with(Vec::new).push(dep);
                }
            }
            Err(e) => {
                log::error!(
                    "Failed to get declared dependencies from {:?}: {}",
                    buildsystem,
                    e
                );
            }
        }

        if !deps.is_empty() {
            log::info!("  Declared dependencies:");
            for (category, deps) in deps {
                for dep in deps {
                    log::info!("    {}: {:?}", category, dep);
                }
            }
        }

        let outputs = match buildsystem.get_declared_outputs(session, fixers) {
            Ok(outputs) => outputs,
            Err(e) => {
                log::error!(
                    "Failed to get declared outputs from {:?}: {}",
                    buildsystem,
                    e
                );
                continue;
            }
        };

        if !outputs.is_empty() {
            log::info!("  Outputs:");
            for output in outputs {
                log::info!("    {:?}", output);
            }
        }
    }

    Ok(())
}
