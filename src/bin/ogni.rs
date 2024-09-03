use clap::{Parser, Subcommand};
use ognibuild::analyze::AnalyzedError;
use ognibuild::buildsystem::{detect_buildsystems, BuildSystem, DependencyCategory, Error};
use ognibuild::dependency::Dependency;
use ognibuild::fix_build::BuildFixer;
use ognibuild::installer::{
    auto_installer, select_installers, Error as InstallerError, Explanation, InstallationScope,
    Installer,
};
use ognibuild::session::Session;
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
use url::Url;

#[derive(Parser)]
struct ExecArgs {
    #[clap(name = "subargv", trailing_var_arg = true)]
    subargv: Vec<String>,
}

#[derive(Parser)]
struct InstallArgs {
    #[clap(long)]
    prefix: Option<PathBuf>,
}

#[derive(Subcommand)]
enum Command {
    #[clap(name = "dist")]
    Dist,
    #[clap(name = "build")]
    Build,
    #[clap(name = "clean")]
    Clean,
    #[clap(name = "test")]
    Test,
    #[clap(name = "info")]
    Info,
    #[clap(name = "verify")]
    Verify,
    #[clap(name = "exec")]
    Exec(ExecArgs),
    #[clap(name = "install")]
    Install(InstallArgs),
}

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Command,

    #[clap(long, short, default_value = ".")]
    directory: String,

    #[cfg(target_os = "linux")]
    #[clap(long)]
    schroot: Option<String>,

    #[clap(long, short, default_value = "auto", use_value_delimiter = true)]
    installer: Vec<String>,

    #[clap(long, hide = true)]
    apt: bool,

    #[clap(long, hide = true)]
    native: bool,

    #[clap(long)]
    /// Explain what needs to be done rather than making changes
    explain: bool,

    #[clap(long)]
    /// Ignore declared dependencies, follow build errors only
    ignore_declared_dependencies: bool,

    #[clap(long)]
    /// Scope to install in
    installation_scope: Option<InstallationScope>,

    #[clap(long, env = "OGNIBUILD_DEPS")]
    /// ognibuild dep server to use
    dep_server_url: Option<url::Url>,

    #[clap(long)]
    /// Print more verbose output
    debug: bool,
}

fn explain_missing_deps(
    session: &dyn Session,
    installer: &dyn Installer,
    scope: InstallationScope,
    deps: &[&dyn Dependency],
) -> Result<Vec<Explanation>, Error> {
    if deps.is_empty() {
        return Ok(vec![]);
    }
    let missing = deps
        .into_iter()
        .filter(|dep| !dep.present(session))
        .collect::<Vec<_>>();
    if missing.is_empty() {
        return Ok(vec![]);
    }
    Ok(missing
        .into_iter()
        .map(|dep| installer.explain(*dep, scope))
        .collect::<Result<_, _>>()?)
}

fn explain_necessary_declared_dependencies(
    session: &dyn Session,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    buildsystems: &[&dyn BuildSystem],
    categories: &[DependencyCategory],
    scope: InstallationScope,
) -> Result<Vec<Explanation>, Error> {
    let mut relevant: Vec<Box<dyn Dependency>> = vec![];
    for buildsystem in buildsystems {
        let declared_deps = buildsystem.get_declared_dependencies(session, Some(fixers))?;
        for (category, dep) in declared_deps {
            if categories.contains(&category) {
                relevant.push(dep);
            }
        }
    }
    explain_missing_deps(
        session,
        installer,
        scope,
        relevant
            .iter()
            .map(|d| d.as_ref())
            .collect::<Vec<_>>()
            .as_slice(),
    )
}

fn install_necessary_declared_dependencies(
    session: &dyn Session,
    installer: &dyn Installer,
    scope: InstallationScope,
    fixers: &[&dyn BuildFixer<InstallerError>],
    buildsystems: &[&dyn BuildSystem],
    categories: &[DependencyCategory],
) -> Result<(), Error> {
    for buildsystem in buildsystems {
        buildsystem.install_declared_dependencies(
            categories,
            scope,
            session,
            installer,
            Some(fixers),
        )?;
    }
    Ok(())
}

fn run_action(
    session: &dyn Session,
    scope: InstallationScope,
    external_dir: &Path,
    installer: &dyn Installer,
    fixers: &[&dyn BuildFixer<InstallerError>],
    args: &Args,
) -> Result<(), Error> {
    if let Command::Exec(ExecArgs { subargv }) = &args.command {
        ognibuild::fix_build::run_fixing_problems::<_, Error>(
            fixers,
            None,
            session,
            subargv
                .iter()
                .map(|s| s.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
            false,
            None,
            None,
            None,
        )?;
        return Ok(());
    }
    let mut log_manager = ognibuild::logs::NoLogManager;
    let bss = detect_buildsystems(external_dir);
    if !args.ignore_declared_dependencies {
        let categories = match args.command {
            Command::Dist => vec![],
            Command::Build => vec![DependencyCategory::Universal, DependencyCategory::Build],
            Command::Clean => vec![],
            Command::Install(_) => vec![DependencyCategory::Universal, DependencyCategory::Build],
            Command::Test => vec![
                DependencyCategory::Universal,
                DependencyCategory::Build,
                DependencyCategory::Test,
            ],
            Command::Info => vec![],
            Command::Verify => vec![
                DependencyCategory::Universal,
                DependencyCategory::Build,
                DependencyCategory::Test,
            ],
            Command::Exec(_) => vec![],
        };
        if !categories.is_empty() {
            log::info!("Checking that declared dependencies are present");
            if !args.explain {
                match install_necessary_declared_dependencies(
                    session,
                    installer,
                    scope,
                    fixers,
                    bss.iter()
                        .map(|bs| bs.as_ref())
                        .collect::<Vec<_>>()
                        .as_slice(),
                    &categories,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        log::info!("Unable to install declared dependencies",);
                        return Err(e);
                    }
                }
            } else {
                match explain_necessary_declared_dependencies(
                    session,
                    installer,
                    fixers,
                    bss.iter()
                        .map(|bs| bs.as_ref())
                        .collect::<Vec<_>>()
                        .as_slice(),
                    &categories,
                    scope,
                ) {
                    Ok(explanations) => {
                        for explanation in explanations {
                            log::info!("{}", explanation);
                        }
                    }
                    Err(e) => {
                        log::info!("Unable to explain declared dependencies",);
                        return Err(e);
                    }
                }
            }
        }
    }

    match &args.command {
        Command::Exec(..) => unreachable!(),
        Command::Dist => {
            ognibuild::actions::dist::run_dist(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                Path::new("."),
                false,
                &mut log_manager,
            )?;
        }
        Command::Build => {
            ognibuild::actions::build::run_build(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
            )?;
        }
        Command::Clean => {
            ognibuild::actions::clean::run_clean(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
            )?;
        }
        Command::Install(install_args) => {
            ognibuild::actions::install::run_install(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
                scope,
                install_args.prefix.as_deref(),
            )?;
        }
        Command::Test => {
            ognibuild::actions::test::run_test(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
            )?;
        }
        Command::Info => {
            ognibuild::actions::info::run_info(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                Some(fixers),
            )?;
        }
        Command::Verify => {
            ognibuild::actions::build::run_build(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
            )?;
            ognibuild::actions::test::run_test(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                &mut log_manager,
            )?;
        }
    }
    Ok(())
}

fn main() -> Result<(), i32> {
    let mut args = Args::parse();

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

    #[cfg(target_os = "linux")]
    let mut session: Box<dyn Session> = if let Some(schroot) = args.schroot.as_ref() {
        Box::new(ognibuild::session::schroot::SchrootSession::new(schroot, None).unwrap())
    } else {
        Box::new(ognibuild::session::plain::PlainSession::new())
    };

    #[cfg(not(target_os = "linux"))]
    let mut session: Box<dyn Session> = Box::new(ognibuild::session::plain::PlainSession::new());

    let url = if let Ok(url) = args.directory.parse::<url::Url>() {
        url
    } else {
        let p = Path::new(&args.directory);
        url::Url::from_directory_path(&p.canonicalize().unwrap()).unwrap()
    };
    let mut td = None;
    // TODO(jelmer): Get a list of supported schemes from breezy?
    let project = if ["git", "http", "https", "ssh"].contains(&url.scheme()) {
        let b = breezyshim::branch::open(&url).unwrap();
        log::info!("Cloning {}", args.directory);
        td = Some(tempfile::tempdir().unwrap());
        let to_dir = b
            .controldir()
            .sprout(
                Url::from_directory_path(td.as_ref().unwrap().path()).unwrap(),
                None,
                Some(true),
                None,
                None,
            )
            .unwrap();
        let wt = to_dir.open_workingtree().unwrap();
        session.project_from_vcs(&wt, None, None).unwrap()
    } else {
        let directory = if url.scheme() == "file" {
            Path::new(url.path()).to_path_buf()
        } else {
            PathBuf::from(args.directory.clone())
        };
        log::info!("Preparing directory {}", directory.display());
        session.project_from_directory(&directory, None).unwrap()
    };
    session.chdir(&project.internal_path()).unwrap();
    std::env::set_current_dir(&project.external_path()).unwrap();

    if !session.is_temporary() && matches!(args.command, Command::Info) {
        args.explain = true;
    }

    if args.apt {
        args.installer = vec!["apt".to_string()];
    }

    if args.native {
        args.installer = vec!["native".to_string()];
    }

    let scope = if let Some(scope) = args.installation_scope {
        scope
    } else if args.explain {
        InstallationScope::Global
    } else {
        ognibuild::installer::auto_installation_scope(session.as_ref())
    };

    let installer: Box<dyn Installer> = if args.installer == ["auto"] {
        auto_installer(session.as_ref(), scope, args.dep_server_url.as_ref())
    } else {
        select_installers(
            session.as_ref(),
            args.installer
                .iter()
                .map(|x| x.as_str())
                .collect::<Vec<_>>()
                .as_slice(),
            args.dep_server_url.as_ref(),
        )
        .unwrap()
    };

    let fixers: Vec<Box<dyn BuildFixer<InstallerError>>> = if !args.explain {
        vec![Box::new(ognibuild::fixers::InstallFixer::new(
            installer.as_ref(),
            scope,
        ))]
    } else {
        vec![]
    };

    match run_action(
        session.as_ref(),
        scope,
        project.external_path(),
        installer.as_ref(),
        fixers
            .iter()
            .map(|f| f.as_ref())
            .collect::<Vec<_>>()
            .as_slice(),
        &args,
    ) {
        Ok(_) => {}
        Err(Error::NoBuildSystemDetected) => {
            log::info!("No build tools found.");
            return Err(1);
        }
        Err(Error::DependencyInstallError(e)) => {
            log::info!("Dependency installation failed: {}", e);
            return Err(1);
        }
        Err(Error::Unimplemented) => {
            log::info!("This command is not yet implemented.");
            return Err(1);
        }
        Err(Error::Error(AnalyzedError::Unidentified { .. })) => {
            log::info!(
                "If there is a clear indication of a problem in the build log, please consider filing a request to update the patterns in buildlog-consultant at https://github.com/jelmer/buildlog-consultant/issues/new");
            return Err(1);
        }
        Err(Error::Error(AnalyzedError::Detailed { error, .. })) => {
            log::info!("Detailed error: {}", error);
            log::info!(
                "Please consider filing a bug report at https://github.com/jelmer/ognibuild/issues/new"
            );
        }
        Err(e) => {
            log::info!("Error: {}", e);
            return Err(1);
        }
    }

    std::mem::drop(td);
    Ok(())
}
