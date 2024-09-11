use clap::Parser;
use ognibuild::buildsystem::{detect_buildsystems, DependencyCategory};
use ognibuild::debian::apt::{dependency_to_deb_dependency, AptManager};
use ognibuild::dependencies::debian::{
    default_tie_breakers, DebianDependency, DebianDependencyCategory,
};
use ognibuild::dependency::Dependency;
use ognibuild::session::plain::PlainSession;
use ognibuild::session::Session;
use std::collections::HashMap;
use std::io::Write;
use std::path::PathBuf;

#[derive(Parser)]
struct Args {
    #[clap(long)]
    detailed: bool,

    directory: PathBuf,

    #[clap(long)]
    debug: bool,
}

fn main() -> Result<(), i32> {
    let args = Args::parse();
    let mut session = PlainSession::new();

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

    session.chdir(&args.directory).unwrap();

    let bss = detect_buildsystems(&args.directory);

    if bss.is_empty() {
        eprintln!("No build tools found");
        std::process::exit(1);
    }

    log::debug!("Detected buildsystems: {:?}", bss);

    let mut deps: HashMap<DependencyCategory, Vec<Box<dyn Dependency>>> = HashMap::new();

    for buildsystem in bss {
        match buildsystem.get_declared_dependencies(&session, Some(&[])) {
            Ok(declared_reqs) => {
                for (stage, req) in declared_reqs {
                    deps.entry(stage).or_default().push(req);
                }
            }
            Err(e) => {
                log::warn!(
                    "Unable to get dependencies from buildsystem {:?}, skipping",
                    buildsystem
                );
                continue;
            }
        }
    }

    let tie_breakers = default_tie_breakers(&session);

    let apt = AptManager::new(&mut session, None);

    if args.detailed {
        let mut unresolved = false;
        for (stage, deps) in deps.iter() {
            log::info!("Stage: {}", stage);
            for dep in deps {
                if let Some(deb_dep) =
                    dependency_to_deb_dependency(&apt, dep.as_ref(), &tie_breakers).unwrap()
                {
                    log::info!("Dependency: {:?} → {}", dep, deb_dep.relation_string());
                } else {
                    log::warn!("Dependency: {:?} → ??", dep);
                    unresolved = true;
                }
            }
            log::info!("");
        }
        if unresolved {
            Err(1)
        } else {
            Ok(())
        }
    } else {
        let mut dep_depends: HashMap<DebianDependencyCategory, Vec<DebianDependency>> =
            HashMap::new();
        let mut unresolved = vec![];
        for (stage, reqs) in deps.iter() {
            for dep in reqs {
                if let Some(deb_dep) =
                    dependency_to_deb_dependency(&apt, dep.as_ref(), &tie_breakers).unwrap()
                {
                    match stage {
                        DependencyCategory::Universal => {
                            dep_depends
                                .entry(DebianDependencyCategory::Build)
                                .or_default()
                                .push(deb_dep.clone());
                            dep_depends
                                .entry(DebianDependencyCategory::Runtime)
                                .or_default()
                                .push(deb_dep);
                        }
                        DependencyCategory::Build => {
                            dep_depends
                                .entry(DebianDependencyCategory::Build)
                                .or_default()
                                .push(deb_dep);
                        }
                        DependencyCategory::Runtime => {
                            dep_depends
                                .entry(DebianDependencyCategory::Runtime)
                                .or_default()
                                .push(deb_dep);
                        }
                        DependencyCategory::BuildExtra(name) => {
                            // TODO: handle build extra: build profile?
                        }
                        DependencyCategory::Test => {
                            dep_depends
                                .entry(DebianDependencyCategory::Test("test".to_string()))
                                .or_default()
                                .push(deb_dep);
                        }
                        DependencyCategory::Dev => {}
                        DependencyCategory::RuntimeExtra(name) => {
                            // TODO: handle runtime extra
                        }
                    }
                } else {
                    unresolved.push(dep);
                }
            }
        }
        for (category, deps) in dep_depends.iter() {
            match category {
                DebianDependencyCategory::Build => {
                    log::info!(
                        "Build-Depends: {}",
                        deps.iter()
                            .map(|d| d.relation_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                DebianDependencyCategory::Runtime => {
                    log::info!(
                        "Depends: {}",
                        deps.iter()
                            .map(|d| d.relation_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                DebianDependencyCategory::Test(test) => {
                    log::info!(
                        "Test-Depends ({}): {}",
                        test,
                        deps.iter()
                            .map(|d| d.relation_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
                DebianDependencyCategory::Install => {
                    log::info!(
                        "Pre-Depends: {}",
                        deps.iter()
                            .map(|d| d.relation_string())
                            .collect::<Vec<_>>()
                            .join(", ")
                    );
                }
            }
        }
        if !unresolved.is_empty() {
            log::warn!("Unable to find apt packages for the following dependencies:");
            for req in unresolved {
                log::warn!("* {:?}", req);
            }
            Err(1)
        } else {
            Ok(())
        }
    }
}
