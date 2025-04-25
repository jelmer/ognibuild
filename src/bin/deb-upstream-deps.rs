use breezyshim::error::Error as BrzError;
use breezyshim::workingtree;
use clap::Parser;
use debian_analyzer::editor::{Editor, MutableTreeEdit};
use debian_control::lossless::relations::Relations;
use ognibuild::session::Session;
use std::io::Write;
use std::path::PathBuf;

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
        Err(e @ BrzError::NotBranchError { .. }) => {
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
    let project = session.project_from_vcs(&wt, None, None).unwrap();
    for (bs_subpath, bs) in
        ognibuild::buildsystem::scan_buildsystems(&wt.abspath(&subpath).unwrap())
    {
        session
            .chdir(&project.internal_path().join(&bs_subpath))
            .unwrap();

        let (bs_build_deps, bs_test_deps) =
            ognibuild::debian::upstream_deps::get_project_wide_deps(session.as_ref(), bs.as_ref());
        build_deps.extend(bs_build_deps);
        test_deps.extend(bs_test_deps);
    }
    if !build_deps.is_empty() {
        println!(
            "Build-Depends: {}",
            build_deps
                .iter()
                .map(|x| x.relation_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if !test_deps.is_empty() {
        println!(
            "Test-Depends: {}",
            test_deps
                .iter()
                .map(|x| x.relation_string())
                .collect::<Vec<_>>()
                .join(", ")
        );
    }
    if args.update {
        let edit = wt
            .edit_file::<debian_control::Control>(&subpath.join("debian/control"), true, true)
            .unwrap();

        let mut source = edit.source().unwrap();

        for build_dep in build_deps {
            for entry in build_dep.iter() {
                let mut relations = source.build_depends().unwrap_or_else(Relations::new);
                let old_str = relations.to_string();
                debian_analyzer::relations::ensure_relation(&mut relations, entry);
                if old_str != relations.to_string() {
                    log::info!("Bumped to {}", relations);
                    source.set_build_depends(&relations);
                }
            }
        }

        edit.commit().unwrap();
    }
    Ok(())
}
