use crate::debian::build::BUILD_LOG_FILENAME;
use crate::debian::build::{attempt_build, BuildOnceError, BuildOnceResult};
use crate::debian::context::Error;
use crate::debian::context::Phase;
pub use crate::fix_build::InterimError;
use breezyshim::error::Error as BrzError;
use breezyshim::workingtree::WorkingTree;
use breezyshim::workspace::reset_tree;
use buildlog_consultant::Match;
use buildlog_consultant::Problem;
use std::path::{Path, PathBuf};

/// Rescue a build log and store it in the users' cache directory
pub fn rescue_build_log(
    output_directory: &Path,
    tree: Option<&WorkingTree>,
) -> Result<(), std::io::Error> {
    let xdg_cache_dir = match dirs::cache_dir() {
        Some(dir) => dir,
        None => {
            log::warn!("Unable to determine cache directory, not saving build log.");
            return Err(std::io::Error::new(
                std::io::ErrorKind::NotFound,
                "Unable to find cache directory",
            ));
        }
    };
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

/// Attempt to resolve a build error by applying appropriate fixers.
///
/// This function finds and applies fixers that can handle the given problem
/// in the specified build phase.
///
/// # Arguments
/// * `problem` - The build problem to fix
/// * `phase` - The build phase in which the problem occurred
/// * `fixers` - List of available fixers to try
///
/// # Returns
/// * `Ok(true)` if a fixer successfully resolved the issue
/// * `Ok(false)` if no applicable fixer was found
/// * `Err(InterimError)` if a fixer encountered an error
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
        /// The return code of the failed command
        retcode: i32,
        /// The output lines from the command
        lines: Vec<String>,
        /// Optional secondary information about the error
        secondary: Option<Box<dyn Match>>,
        /// The build phase in which the error occurred
        phase: Option<Phase>,
    },

    /// The build phase could not be determined
    MissingPhase,

    /// An error occurred while resetting the tree
    ResetTree(BrzError),

    /// Another error raised specifically by the callback function that is not fixable.
    Other(Error),
}

/// Build a Debian package incrementally, with automatic error fixing.
///
/// This function attempts to build a Debian package, and if the build fails,
/// it tries to fix the errors automatically using the provided fixers.
/// It will retry the build after each fix until either the build succeeds,
/// or it encounters an unfixable error.
///
/// # Arguments
/// * `local_tree` - Working tree containing the package source
/// * `suffix` - Optional suffix for the binary package version
/// * `build_suite` - Optional distribution suite to build for
/// * `output_directory` - Directory where build artifacts will be stored
/// * `build_command` - Command to use for building (e.g., "dpkg-buildpackage")
/// * `fixers` - List of fixers to apply if build errors are encountered
/// * `build_changelog_entry` - Optional changelog entry to add before building
/// * `max_iterations` - Maximum number of fix attempts before giving up
/// * `subpath` - Path within the working tree where the package is located
/// * `source_date_epoch` - Optional timestamp for reproducible builds
/// * `apt_repository` - Optional URL of an APT repository to use
/// * `apt_repository_key` - Optional GPG key for the APT repository
/// * `extra_repositories` - Optional additional APT repositories to use
/// * `run_gbp_dch` - Whether to run git-buildpackage's dch command
///
/// # Returns
/// * `Ok(BuildOnceResult)` if the build succeeded
/// * `Err(IterateBuildError)` if the build failed and could not be fixed
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
                stage: _,
                phase,
                retcode,
                command: _,
                description: _,
            }) => {
                log::warn!("Build failed with unidentified error. Giving up.");
                return Err(IterateBuildError::Unidentified {
                    phase,
                    retcode,
                    lines: vec![],
                    secondary: None,
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
                    .map_err(IterateBuildError::ResetTree)?;

                match resolve_error(error.as_ref(), &phase, fixers) {
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
                            phase: Some(phase),
                            retcode,
                            lines,
                            secondary,
                        });
                    }
                }
                fixed_errors.push((error, phase));
            }
        }
    }
}
