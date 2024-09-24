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

#[cfg(test)]
mod tests {
    use super::*;
    mod test_resolve_error {
        use super::*;
        use crate::debian::apt::AptManager;
        use crate::debian::context::DebianPackagingContext;
        use crate::debian::file_search::MemoryAptSearcher;
        use crate::session::plain::PlainSession;
        use breezyshim::commit::NullCommitReporter;
        use breezyshim::controldir::{create_standalone_workingtree, ControlDirFormat};
        use breezyshim::tree::Tree;
        use buildlog_consultant::problems::common::*;
        use debian_control::lossless::Control;
        use std::collections::HashMap;
        use std::path::{Path, PathBuf};
        use test_log::test;

        fn setup(path: &Path) -> WorkingTree {
            let tree = create_standalone_workingtree(&path, &ControlDirFormat::default()).unwrap();

            std::fs::create_dir_all(path.join("debian")).unwrap();
            std::fs::write(
                path.join("debian/control"),
                r#"Source: blah
Build-Depends: libc6

Package: python-blah
Depends: ${python3:Depends}
Description: A python package
 Foo
"#,
            )
            .unwrap();
            std::fs::write(
                path.join("debian/changelog"),
                r#"blah (0.1) UNRELEASED; urgency=medium

  * Initial release. (Closes: #XXXXXX)

 -- ognibuild <ognibuild@jelmer.uk>  Sat, 04 Apr 2020 14:12:13 +0000
"#,
            )
            .unwrap();
            tree.add(&[
                Path::new("debian"),
                Path::new("debian/control"),
                Path::new("debian/changelog"),
            ])
            .unwrap();
            tree.build_commit()
                .message("Initial commit")
                .committer("ognibuild <ognibuild@jelmer.uk>")
                .commit()
                .unwrap();

            tree
        }

        fn resolve(
            tree: &WorkingTree,
            error: &dyn Problem,
            phase: &Phase,
            apt_files: HashMap<PathBuf, String>,
        ) -> bool {
            let session = PlainSession::new();
            let apt = AptManager::new(&session, None);
            apt.set_searchers(vec![Box::new(MemoryAptSearcher::new(apt_files))]);

            let context = DebianPackagingContext::new(
                tree.clone(),
                Path::new(""),
                Some(("ognibuild".to_owned(), "ognibuild@jelmer.uk".to_owned())),
                true,
                Some(Box::new(NullCommitReporter::new())),
            );

            let mut fixers: Vec<Box<dyn DebianBuildFixer>> =
                crate::debian::fixers::versioned_package_fixers(&session, &context, &apt);
            fixers.extend(crate::debian::fixers::apt_fixers(&apt, &context));
            resolve_error(
                error,
                phase,
                &fixers.iter().map(|f| f.as_ref()).collect::<Vec<_>>(),
            )
            .unwrap()
        }

        fn get_build_deps(tree: &dyn Tree) -> String {
            let content = tree.get_file_text(Path::new("debian/control")).unwrap();

            let content = String::from_utf8(content).unwrap();

            let control: Control = content.parse().unwrap();

            control
                .source()
                .unwrap()
                .build_depends()
                .unwrap()
                .to_string()
        }

        #[test]
        fn test_missing_command_unknown() {
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());
            assert!(!resolve(
                &tree,
                &MissingCommand("acommandthatdoesnotexist".to_string()),
                &Phase::Build,
                HashMap::new()
            ));
        }

        #[test]
        fn test_missing_command_brz() {
            let env = breezyshim::testing::TestEnv::new();
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/bin/b") => "bash".to_string(),
                PathBuf::from("/usr/bin/brz") => "brz".to_string(),
                PathBuf::from("/usr/bin/brzier") => "bash".to_string(),
            };
            assert!(resolve(
                &tree,
                &MissingCommand("brz".to_string()),
                &Phase::Build,
                apt_files.clone()
            ));
            assert_eq!("libc6, brz", get_build_deps(&tree));
            let rev = tree
                .branch()
                .repository()
                .get_revision(&tree.branch().last_revision());
            assert_eq!(
                "Add missing build dependency on brz.\n",
                rev.unwrap().message
            );
            // Now that the dependency is added, we should not try to add it again.
            assert!(!resolve(
                &tree,
                &MissingCommand("brz".to_owned()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, brz", get_build_deps(&tree));

            std::mem::drop(env);
        }

        #[test]
        fn test_missing_command_ps() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/bin/ps") => "procps".to_string(),
                PathBuf::from("/usr/bin/pscal") => "xcal".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());
            assert!(resolve(
                &tree,
                &MissingCommand("ps".to_owned()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, procps", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_ruby_file() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/lib/ruby/vendor_ruby/rake/testtask.rb") => "rake".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingRubyFile::new("rake/testtask".to_string()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, rake", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_ruby_file_from_gem() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/share/rubygems-integration/all/gems/activesupport-5.2.3/lib/active_support/core_ext/string/strip.rb") => "ruby-activesupport".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingRubyFile::new("active_support/core_ext/string/strip".to_string()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, ruby-activesupport", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_ruby_gem() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/share/rubygems-integration/all/specifications/bio-1.5.2.gemspec") => "ruby-bio".to_string(),
                PathBuf::from("/usr/share/rubygems-integration/all/specifications/bio-2.0.2.gemspec") => "ruby-bio".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingRubyGem::simple("bio".to_string()),
                &Phase::Build,
                apt_files.clone()
            ));
            assert_eq!("libc6, ruby-bio", get_build_deps(&tree));
            assert!(resolve(
                &tree,
                &MissingRubyGem::new("bio".to_string(), Some("2.0.3".to_string())),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, ruby-bio (>= 2.0.3)", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_perl_module() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/share/perl5/App/cpanminus/fatscript.pm") => "cpanminus".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingPerlModule {
                    filename: Some("App/cpanminus/fatscript.pm".to_string()),
                    module: "App::cpanminus::fatscript".to_string(),
                    minimum_version: None,
                    inc: Some(vec![
                        "/<<PKGBUILDDIR>>/blib/lib".to_string(),
                        "/<<PKGBUILDDIR>>/blib/arch".to_string(),
                        "/etc/perl".to_string(),
                        "/usr/local/lib/x86_64-linux-gnu/perl/5.30.0".to_string(),
                        "/usr/local/share/perl/5.30.0".to_string(),
                        "/usr/lib/x86_64-linux-gnu/perl5/5.30".to_string(),
                        "/usr/share/perl5".to_string(),
                        "/usr/lib/x86_64-linux-gnu/perl/5.30".to_string(),
                        "/usr/share/perl/5.30".to_string(),
                        "/usr/local/lib/site_perl".to_string(),
                        "/usr/lib/x86_64-linux-gnu/perl-base".to_string(),
                        ".".to_string(),
                    ]),
                },
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, cpanminus", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_pkg_config() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/lib/x86_64-linux-gnu/pkgconfig/xcb-xfixes.pc") => "libxcb-xfixes0-dev".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingPkgConfig::simple("xcb-xfixes".to_string()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, libxcb-xfixes0-dev", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_pkg_config_versioned() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/lib/x86_64-linux-gnu/pkgconfig/xcb-xfixes.pc") => "libxcb-xfixes0-dev".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingPkgConfig::new("xcb-xfixes".to_string(), Some("1.0".to_string())),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, libxcb-xfixes0-dev (>= 1.0)", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_python_module() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/lib/python3/dist-packages/m2r.py") => "python3-m2r".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingPythonModule::simple("m2r".to_string()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, python3-m2r", get_build_deps(&tree));
        }

        #[test]
        fn test_missing_go_package() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/share/gocode/src/github.com/chzyer/readline/utils_test.go") => "golang-github-chzyer-readline-dev".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingGoPackage {
                    package: "github.com/chzyer/readline".to_string()
                },
                &Phase::Build,
                apt_files
            ));
            assert_eq!(
                "libc6, golang-github-chzyer-readline-dev",
                get_build_deps(&tree)
            );
        }

        #[test]
        fn test_missing_vala_package() {
            let apt_files = maplit::hashmap! {
                PathBuf::from("/usr/share/vala-0.48/vapi/posix.vapi") => "valac-0.48-vapi".to_string(),
            };
            let td = tempfile::tempdir().unwrap();
            let tree = setup(td.path());

            assert!(resolve(
                &tree,
                &MissingValaPackage("posix".to_string()),
                &Phase::Build,
                apt_files
            ));
            assert_eq!("libc6, valac-0.48-vapi", get_build_deps(&tree));
        }
    }
}
