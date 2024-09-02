use crate::debian::build::BUILD_LOG_FILENAME;
use crate::debian::build::{attempt_build, BuildOnceError, BuildOnceResult};
use crate::debian::context::Error;
use crate::debian::context::Phase;
use crate::fix_build::InterimError;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::WorkingTree;
use breezyshim::workspace::reset_tree;
use buildlog_consultant::Match;
use buildlog_consultant::Problem;
use std::path::{Path, PathBuf};

pub fn rescue_build_log(
    output_directory: &Path,
    tree: Option<&WorkingTree>,
) -> Result<(), std::io::Error> {
    let xdg_cache_dir = std::env::var("XDG_CACHE_HOME").ok().map_or_else(
        || std::env::home_dir().unwrap().join(".cache"),
        PathBuf::from,
    );
    let buildlogs_dir = xdg_cache_dir.join("ognibuild/buildlogs");
    std::fs::create_dir_all(&buildlogs_dir)?;

    let target_log_file = buildlogs_dir.join(format!(
        "{}-{}.log",
        tree.map_or_else(|| PathBuf::from("build"), |t| t.basedir())
            .display(),
        chrono::Local::now().format("%Y-%m-%d_%H%M%s"),
    ));
    std::fs::copy(output_directory.join("build.log"), &target_log_file)?;
    log::info!("Build log available in {}", target_log_file.display());

    Ok(())
}

/// A fixer is a struct that can resolve a specific type of problem.
pub trait DebianBuildFixer: std::fmt::Debug + std::fmt::Display {
    /// Check if this fixer can potentially resolve the given problem.
    fn can_fix(&self, problem: &dyn Problem) -> bool;

    /// Attempt to resolve the given problem.
    fn fix(&self, problem: &dyn Problem, phase: &Phase) -> Result<bool, InterimError<Error>>;
}

pub fn resolve_error(
    problem: &dyn Problem,
    phase: &Phase,
    fixers: &[&dyn DebianBuildFixer],
) -> Result<bool, InterimError<Error>> {
    let relevant_fixers = fixers
        .iter()
        .filter(|fixer| fixer.can_fix(problem))
        .collect::<Vec<_>>();
    if relevant_fixers.is_empty() {
        log::warn!("No fixer found for {:?}", problem);
        return Ok(false);
    }
    for fixer in relevant_fixers {
        log::info!("Attempting to use fixer {} to address {:?}", fixer, problem);
        let made_changes = fixer.fix(problem, phase)?;
        if made_changes {
            return Ok(true);
        }
    }
    Ok(false)
}

/// Error result from repeatedly running and attemptin to fix issues.
#[derive(Debug)]
pub enum IterateBuildError {
    /// The limit of fixing attempts was reached.
    FixerLimitReached(usize),

    /// A problem was detected that was recognized but could not be fixed.
    Persistent(Phase, Box<dyn Problem>),

    /// An error that we could not identify.
    Unidentified {
        retcode: i32,
        lines: Vec<String>,
        secondary: Option<Box<dyn Match>>,
        phase: Option<Phase>,
    },

    MissingPhase,

    ResetTree(BrzError),

    /// Another error raised specifically by the callback function that is not fixable.
    Other(Error),
}

pub fn build_incrementally(
    local_tree: &WorkingTree,
    suffix: Option<&str>,
    build_suite: Option<&str>,
    output_directory: &Path,
    build_command: &str,
    fixers: &[&dyn DebianBuildFixer],
    build_changelog_entry: Option<&str>,
    max_iterations: Option<usize>,
    subpath: &Path,
    source_date_epoch: Option<chrono::DateTime<chrono::Utc>>,
    apt_repository: Option<&str>,
    apt_repository_key: Option<&str>,
    extra_repositories: Option<Vec<&str>>,
    run_gbp_dch: bool,
) -> Result<BuildOnceResult, IterateBuildError> {
    let mut fixed_errors: Vec<(Box<dyn Problem>, Phase)> = vec![];
    log::info!("Using fixers: {:?}", fixers);
    loop {
        match attempt_build(
            local_tree,
            suffix,
            build_suite,
            output_directory,
            build_command,
            build_changelog_entry,
            subpath,
            source_date_epoch,
            run_gbp_dch,
            apt_repository,
            apt_repository_key,
            extra_repositories.as_ref(),
        ) {
            Ok(result) => {
                return Ok(result);
            }
            Err(BuildOnceError::Unidentified {
                stage,
                phase,
                retcode,
                command,
                description: _,
            }) => {
                log::warn!("Build failed with unidentified error. Giving up.");
                return Err(IterateBuildError::Unidentified {
                    phase,
                    retcode,
                    lines: todo!(),
                    secondary: todo!(),
                });
            }
            Err(BuildOnceError::Detailed { phase, error, .. }) => {
                if phase.is_none() {
                    log::info!("No relevant context, not making any changes.");
                    return Err(IterateBuildError::MissingPhase);
                }
                let phase = phase.unwrap();
                if fixed_errors.iter().any(|(e, p)| e == &error && p == &phase) {
                    log::warn!("Error was still not fixed on second try. Giving up.");
                    return Err(IterateBuildError::Persistent(phase, error));
                }

                if max_iterations
                    .map(|max| fixed_errors.len() >= max)
                    .unwrap_or(false)
                {
                    log::warn!("Max iterations reached. Giving up.");
                    return Err(IterateBuildError::FixerLimitReached(
                        max_iterations.unwrap(),
                    ));
                }
                reset_tree(local_tree, None, Some(subpath))
                    .map_err(|e| IterateBuildError::ResetTree(e))?;

                match resolve_error(
                    error.as_ref(),
                    &phase,
                    fixers
                ) {
                    Ok(false) => {
                        log::warn!("Failed to resolve error {:?}. Giving up.", error);
                        return Err(IterateBuildError::Persistent(phase, error));
                    }
                    Ok(true) => {}
                    Err(InterimError::Other(e)) => {
                        return Err(IterateBuildError::Other(e));
                    }
                    Err(InterimError::Recognized(p)) => {
                        if &error != &p {
                            log::warn!("Detected problem while fixing {:?}: {:?}", error, p);
                        }
                        return Err(IterateBuildError::Persistent(phase, error));
                    }
                    Err(InterimError::Unidentified {
                        retcode,
                        lines,
                        secondary,
                    }) => {
                        log::warn!("Recognized error but unable to resolve: {:?}", lines);
                        return Err(IterateBuildError::Unidentified {
                            retcode,
                            lines,
                            phase: Some(phase),
                            secondary,
                        });
                    }
                }
                fixed_errors.push((error, phase));
                crate::logs::rotate_logfile(&output_directory.join(BUILD_LOG_FILENAME)).unwrap();
            }
        }
    }
}
