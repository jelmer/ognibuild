use clap::Parser;
use std::path::PathBuf;
use breezyshim::workingtree;
use ognibuild::session::Session;
use ognibuild::session::plain::PlainSession;
use breezyshim::error::Error as BrzError;
use debian_control::lossless::relations::Relations;
use debian_analyzer::editor::{Editor,MutableTreeEdit};
use std::io::Write;

#[derive(Parser)]
struct Args {
    #[clap(short, long)]
    /// Be verbose
    debug: bool,

    #[clap(short, long)]
    /// Update current package
    update: bool,

    #[clap(short, long, default_value = ".")]
    /// Directory to run in
    directory: PathBuf,
}

fn main() -> Result<(), i8> {
    let args = Args::parse();

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

    let (wt, subpath) = match workingtree::open_containing(&args.directory) {
        Ok((wt, subpath)) => (wt, subpath),
        Err(e@BrzError::NotBranchError { .. }) => {
            log::error!("please run deps in an existing branch: {}", e);
            return Err(1);
        }
        Err(e) => {
            log::error!("error opening working tree: {}", e);
            return Err(1);
        }
    };

    let mut build_deps = vec![];
    let mut test_deps = vec![];

    let mut session: Box<dyn Session> = Box::new(ognibuild::session::plain::PlainSession::new());
    for (bs_subpath, bs) in ognibuild::buildsystem::scan_buildsystems(&wt.abspath(&subpath).unwrap()) {
        let (bs_build_deps, bs_test_deps) = ognibuild::debian::upstream_deps::get_project_wide_deps(session.as_mut(), &wt, &subpath, bs.as_ref(), &bs_subpath
        );
        build_deps.extend(bs_build_deps);
        test_deps.extend(bs_test_deps);
    }
    if !build_deps.is_empty() {
        println!("Build-Depends: {}", build_deps.iter().map(|x| x.relation_string()).collect::<Vec<_>>().join(", "));
    }
    if !test_deps.is_empty() {
        println!("Test-Depends: {}", test_deps.iter().map(|x| x.relation_string()).collect::<Vec<_>>().join(", "));
    }
    if args.update {
        let edit = wt.edit_file::<debian_control::Control>(&subpath.join("debian/control"), true, true).unwrap();

        let mut source = edit.source().unwrap();

        for build_dep in build_deps {
            for entry in build_dep.iter() {
                let mut relations = source.build_depends().unwrap_or_else(|| Relations::new());
                let old_str = relations.to_string();
                debian_analyzer::relations::ensure_relation(&mut relations, entry);
                if old_str != relations.to_string() {
                    log::info!("Bumped to {}", relations.to_string());
                    source.set_build_depends(&relations);
                }
            }
        }

        edit.commit().unwrap();
    }
    Ok(())
}
