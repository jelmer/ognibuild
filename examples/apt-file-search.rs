use clap::Parser;
use ognibuild::debian::file_search::{
    get_apt_contents_file_searcher, get_packages_for_paths, FileSearcher, GENERATED_FILE_SEARCHER,
};
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    /// Search for regex.
    regex: bool,
    /// Path to search for.
    path: Vec<PathBuf>,
    #[clap(short, long)]
    /// Enable debug output.
    debug: bool,
    #[clap(short, long)]
    /// Case insensitive search.
    case_insensitive: bool,
}

pub fn main() -> Result<(), i8> {
    let args: Args = Args::parse();
    env_logger::builder()
        .filter_level(if args.debug {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .init();

    let session = ognibuild::session::plain::PlainSession::new();
    let main_searcher = get_apt_contents_file_searcher(&session).unwrap();
    let searchers: Vec<&dyn FileSearcher> = vec![
        main_searcher.as_ref(),
        &*GENERATED_FILE_SEARCHER as &dyn FileSearcher,
    ];

    let packages = get_packages_for_paths(
        args.path
            .iter()
            .map(|x| x.as_path().to_str().unwrap())
            .collect(),
        searchers.as_slice(),
        args.regex,
        args.case_insensitive,
    );
    for package in packages {
        println!("{}", package);
    }

    Ok(())
}
