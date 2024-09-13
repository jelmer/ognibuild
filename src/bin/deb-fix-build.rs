use clap::Parser;
use ognibuild::debian::build::BuildOnceError;
use ognibuild::debian::fix_build::{rescue_build_log, IterateBuildError};
#[cfg(target_os = "linux")]
use ognibuild::session::schroot::SchrootSession;
use ognibuild::session::{plain::PlainSession, Session};
use std::fmt::Write as _;
use std::io::Write as _;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    /// Suffix to use for test builds
    #[clap(short, long, default_value = "fixbuild1")]
    suffix: String,

    /// Suite to target
    #[clap(short, long, default_value = "unstable")]
    suite: String,

    /// Committer string (name and email)
    #[clap(short, long)]
    committer: Option<String>,

    /// Document changes in the changelog [default: auto-detect]
    #[arg(long, default_value_t = false, conflicts_with = "no_update_changelog")]
    update_changelog: bool,

    /// Do not document changes in the changelog (useful when using e.g. "gbp dch") [default: auto-detect]
    #[arg(long, default_value_t = false, conflicts_with = "update_changelog")]
    no_update_changelog: bool,

    /// Output directory.
    #[clap(short, long)]
    output_directory: Option<PathBuf>,

    /// Build command
    #[clap(short, long, default_value = "sbuild -A -s -v")]
    build_command: String,

    /// Maximum number of issues to attempt to fix before giving up.
    #[clap(short, long, default_value = "10")]
    max_iterations: usize,

    #[cfg(target_os = "linux")]
    /// chroot to use.
    #[clap(short, long)]
    schroot: Option<String>,

    /// ognibuild dep server to use
    #[clap(short, long, env = "OGNIBUILD_DEPS")]
    dep_server_url: Option<String>,

    /// Be verbose
    #[clap(short, long)]
    verbose: bool,

    /// Directory to use
    #[clap(short, long, default_value = ".")]
    directory: PathBuf,
}

fn main() -> Result<(), i32> {
    let args = Args::parse();

    let update_changelog: Option<bool> = if args.update_changelog {
        Some(true)
    } else if args.no_update_changelog {
        Some(false)
    } else {
        None
    };

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.verbose {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    let mut temp_output_dir = None;

    let output_directory = if let Some(output_directory) = &args.output_directory {
        if !output_directory.is_dir() {
            log::error!("output directory {:?} is not a directory", output_directory);
            std::process::exit(1);
        }
        output_directory.clone()
    } else {
        temp_output_dir = Some(tempfile::tempdir().unwrap());
        log::info!("Using output directory {:?}", temp_output_dir);

        temp_output_dir.as_ref().unwrap().path().to_path_buf()
    };

    let (tree, subpath) = breezyshim::workingtree::open_containing(&args.directory).unwrap();

    #[cfg(target_os = "linux")]
    let session = if let Some(schroot) = &args.schroot {
        Box::new(SchrootSession::new(schroot, Some("deb-fix-build")).unwrap()) as Box<dyn Session>
    } else {
        Box::new(PlainSession::new())
    };

    #[cfg(not(target_os = "linux"))]
    let session = Box::new(PlainSession::new());

    let apt = ognibuild::debian::apt::AptManager::new(session.as_ref(), None);

    let committer = if let Some(committer) = &args.committer {
        Some(breezyshim::config::parse_username(committer))
    } else {
        None
    };

    let packaging_context = ognibuild::debian::context::DebianPackagingContext::new(
        tree.clone(),
        &subpath,
        committer,
        args.update_changelog,
        Box::new(breezyshim::commit::NullCommitReporter::new()),
    );

    let fixers = ognibuild::debian::fixers::default_fixers(&packaging_context, &apt);

    match ognibuild::debian::fix_build::build_incrementally(
        &tree,
        Some(&args.suffix),
        Some(&args.suite),
        &output_directory,
        &args.build_command,
        fixers
            .iter()
            .map(|f| f.as_ref())
            .collect::<Vec<_>>()
            .as_slice(),
        None,
        Some(args.max_iterations),
        &subpath,
        None,
        None,
        None,
        None,
        update_changelog == Some(false),
    ) {
        Ok(build_result) => {
            log::info!(
                "Built {} - changes file at {:?}.",
                build_result.version,
                build_result.changes_names,
            );
            return Ok(());
        }
        Err(IterateBuildError::Persistent(phase, error)) => {
            log::error!("Error during {}: {}", phase, error);
            if let Some(output_directory) = args.output_directory {
                rescue_build_log(&output_directory, Some(&tree)).unwrap();
            }
            return Err(1);
        }
        Err(IterateBuildError::Unidentified {
            phase,
            lines,
            secondary,
            ..
        }) => {
            let mut header = if let Some(phase) = phase {
                format!("Error during {}:", phase)
            } else {
                "Error:".to_string()
            };
            if let Some(m) = secondary {
                let linenos = m.linenos();
                write!(
                    header,
                    " on lines {}-{}",
                    linenos[0],
                    linenos[linenos.len() - 1]
                )
                .unwrap();
            }
            header.write_str(":").unwrap();
            log::error!("{}", header);
            for line in lines {
                log::error!("  {}", line);
            }
            if let Some(output_directory) = args.output_directory {
                rescue_build_log(&output_directory, Some(&tree)).unwrap();
            }
            return Err(1);
        }
        Err(IterateBuildError::FixerLimitReached(n)) => {
            log::error!("Fixer limit reached - {} attempts.", n);
            return Err(1);
        }
        Err(IterateBuildError::Other(o)) => {
            log::error!("Error: {}", o);
            return Err(1);
        }
        Err(IterateBuildError::MissingPhase) => {
            log::error!("Missing phase");
            return Err(1);
        }
        Err(IterateBuildError::ResetTree(e)) => {
            log::error!("Error resetting tree: {}", e);
            return Err(1);
        }
    }
}
