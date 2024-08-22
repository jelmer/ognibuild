use clap::Parser;
use ognibuild::debian::build::{BuildOnceError, DEFAULT_BUILDER};
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    suffix: Option<String>,
    #[clap(long, default_value = DEFAULT_BUILDER)]
    build_command: String,
    #[clap(short, long, default_value = "..")]
    output_directory: PathBuf,
    #[clap(short, long)]
    build_suite: Option<String>,
    #[clap(long)]
    debug: bool,
    #[clap(long)]
    build_changelog_entry: Option<String>,
    #[clap(short, long, default_value = ".")]
    /// The directory to build in
    directory: PathBuf,
    /// Use gbp dch to generate the changelog entry
    #[clap(long, default_value = "false")]
    gbp_dch: bool,
}

pub fn main() -> Result<(), i32> {
    let args = Args::parse();
    let dir = args.directory;
    let (wt, subpath) = breezyshim::workingtree::open_containing(&dir).unwrap();

    breezyshim::init();
    breezyshim::plugin::load_plugins();

    env_logger::builder()
        .format(|buf, record| writeln!(buf, "{}", record.args()))
        .filter(
            None,
            if args.debug {
                log::LevelFilter::Debug
            } else {
                log::LevelFilter::Info
            },
        )
        .init();

    log::info!("Using output directory {}", args.output_directory.display());

    if args.suffix.is_some() && args.build_changelog_entry.is_none() {
        log::warn!("--suffix is ignored without --build-changelog-entry");
    }

    if args.build_changelog_entry.is_some() && args.build_suite.is_none() {
        log::error!("--build-changelog-entry requires --build-suite");
        return Err(1);
    }

    let source_date_epoch = std::env::var("SOURCE_DATE_EPOCH")
        .ok()
        .map(|s| s.parse().unwrap());

    match ognibuild::debian::build::attempt_build(
        &wt,
        args.suffix.as_deref(),
        args.build_suite.as_deref(),
        &args.output_directory,
        &args.build_command,
        args.build_changelog_entry.as_deref(),
        &subpath,
        source_date_epoch,
        args.gbp_dch,
        None,
        None,
        None,
    ) {
        Ok(_) => {}
        Err(BuildOnceError::Unidentified {
            phase, description, ..
        }) => {
            if let Some(phase) = phase {
                log::error!("build failed during {}: {}", phase, description);
            } else {
                log::error!("build failed: {}", description);
            }
            return Err(1);
        }
        Err(BuildOnceError::Detailed {
            phase,
            description,
            error,
            ..
        }) => {
            if let Some(phase) = phase {
                log::error!("build failed during {}: {}", phase, description);
            } else {
                log::error!("build failed: {}", description);
            }
            log::info!("error: {:?}", error);
            return Err(1);
        }
    }

    Ok(())
}
