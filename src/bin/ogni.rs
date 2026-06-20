#[cfg(feature = "breezy")]
use breezyshim::branch::Branch;
use clap::{Parser, Subcommand};
use ognibuild::analyze::AnalyzedError;
use ognibuild::buildsystem::{
    detect_buildsystems, supported_buildsystem_names, BuildSystem, DependencyCategory, Error,
};
use ognibuild::dependency::Dependency;
use ognibuild::fix_build::BuildFixer;
use ognibuild::installer::{
    auto_installer, select_installers, Error as InstallerError, Explanation, InstallationScope,
    Installer,
};
use ognibuild::session::{Session, SessionKind};
use std::io::Write;
use std::path::Path;
use std::path::PathBuf;
#[cfg(feature = "breezy")]
use url::Url;

/// Check if network access is disabled via the OGNIBUILD_DISABLE_NET environment variable.
///
/// Network access is disabled if OGNIBUILD_DISABLE_NET is set to "1", "true", "yes", or "on" (case-insensitive).
///
/// # Arguments
/// * `env_getter` - Function to get environment variable value
///
/// # Returns
/// `true` if network access should be disabled, `false` otherwise
fn is_network_disabled_with<F>(env_getter: F) -> bool
where
    F: Fn(&str) -> Option<String>,
{
    match env_getter("OGNIBUILD_DISABLE_NET") {
        Some(val) => {
            let val = val.to_lowercase();
            val == "1" || val == "true" || val == "yes" || val == "on"
        }
        None => false,
    }
}

/// Check if network access is disabled via the OGNIBUILD_DISABLE_NET environment variable.
///
/// Network access is disabled if OGNIBUILD_DISABLE_NET is set to "1", "true", "yes", or "on" (case-insensitive).
///
/// # Returns
/// `true` if network access should be disabled, `false` otherwise
fn is_network_disabled() -> bool {
    is_network_disabled_with(|key| std::env::var(key).ok())
}

#[derive(Parser)]
struct ExecArgs {
    /// Before running the command, install the Debian source package's
    /// Build-Depends (from debian/control) via apt, as for "scip". Lets the
    /// command run with the build environment present without ognibuild's own
    /// build-system dependency detection.
    #[clap(long)]
    apt_build_deps: bool,

    /// After the command, copy a file produced inside the session out to the
    /// host, given as "SESSION_PATH:HOST_PATH" (repeatable). A relative
    /// SESSION_PATH is taken relative to the session's working directory. Useful
    /// for retrieving output (e.g. a SCIP index) from a session that is
    /// otherwise isolated from the host filesystem.
    #[clap(long = "copy-out", value_name = "SESSION_PATH:HOST_PATH")]
    copy_out: Vec<String>,

    #[clap(name = "subargv", trailing_var_arg = true)]
    subargv: Vec<String>,
}

#[derive(Parser)]
struct InstallArgs {
    #[clap(long)]
    prefix: Option<PathBuf>,
}

#[derive(Parser)]
struct ScipArgs {
    /// Path to write a single combined SCIP index file to
    #[clap(long, short, conflicts_with = "output_all")]
    output: Option<PathBuf>,

    /// Directory to write one SCIP index file per build system into,
    /// each named after the indexed language (e.g. python.scip)
    #[clap(long)]
    output_all: Option<PathBuf>,

    /// Before indexing, install the Debian source package's Build-Depends
    /// (from debian/control) via apt. These resolve to concrete packages,
    /// unlike the build system's own declared dependencies, so indexers that
    /// need the build environment present (e.g. scip-python reading setup.py
    /// metadata) work. Requires a session where apt can install packages.
    #[clap(long)]
    apt_build_deps: bool,

    /// Isolate the session from the network while indexing. Indexing normally
    /// needs the network (e.g. to install build dependencies), so it is
    /// allowed by default; pass this to cut it off.
    #[clap(long)]
    offline: bool,
}

#[derive(Parser)]
struct LsifArgs {
    /// Path to write the LSIF index file to
    #[clap(long, short, default_value = "dump.lsif")]
    output: PathBuf,
}

#[derive(Parser)]
struct CacheEnvArgs {
    /// Debian suite to cache (e.g., "sid", "bookworm", "stable")
    #[clap(default_value = "sid")]
    suite: String,

    /// Force re-download even if cached
    #[clap(long)]
    force: bool,

    /// Refresh an existing cached image by running apt upgrade inside it
    #[clap(long, conflicts_with = "force")]
    update: bool,

    /// Additional packages to install into the cached image (repeatable or
    /// comma-separated), e.g. to provide tools the indexers need in-session.
    #[clap(long = "include", value_delimiter = ',')]
    include: Vec<String>,
}

#[derive(Subcommand)]
enum Command {
    #[clap(name = "dist")]
    /// Create a distribution package/tarball
    Dist,
    #[clap(name = "build")]
    /// Build the project
    Build,
    #[clap(name = "clean")]
    /// Clean build artifacts
    Clean,
    #[clap(name = "test")]
    /// Run tests
    Test,
    #[clap(name = "info")]
    /// Display build system information and dependencies
    Info,
    #[clap(name = "verify")]
    /// Build and run tests
    Verify,
    #[clap(name = "exec")]
    /// Execute a command with automatic dependency resolution
    Exec(ExecArgs),
    #[clap(name = "install")]
    /// Install the project
    Install(InstallArgs),
    #[clap(name = "cache-env")]
    /// Cache a Debian cloud image for use with UnshareSession
    CacheEnv(CacheEnvArgs),
    #[clap(name = "scip")]
    /// Generate a SCIP index file for the project
    Scip(ScipArgs),
    #[clap(name = "lsif")]
    /// Generate an LSIF index file for the project
    Lsif(LsifArgs),
}

#[derive(Parser)]
struct Args {
    #[clap(subcommand)]
    command: Option<Command>,

    #[clap(long, short, default_value = ".")]
    directory: String,

    #[cfg(target_os = "linux")]
    #[clap(long)]
    /// schroot chroot to run in (shorthand for --session schroot:<name>)
    schroot: Option<String>,

    #[clap(long)]
    /// Session backend to run in: "plain", "schroot:<name>", or "unshare:<suite>"
    session: Option<SessionKind>,

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

    #[clap(long)]
    /// List all supported build systems
    supported_buildsystems: bool,
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
        .iter()
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
    scopes: &[InstallationScope],
    fixers: &[&dyn BuildFixer<InstallerError>],
    buildsystems: &[&dyn BuildSystem],
    categories: &[DependencyCategory],
) -> Result<(), Error> {
    for buildsystem in buildsystems {
        match buildsystem.install_declared_dependencies(
            categories,
            scopes,
            session,
            installer,
            Some(fixers),
        ) {
            Ok(()) => {}
            // A build system that can't enumerate its declared dependencies
            // (e.g. a bare gemspec) simply has none to install here; the
            // indexer still resolves what it needs on its own.
            Err(Error::Unimplemented) => {
                log::info!(
                    "{} does not support declared dependency discovery; skipping",
                    buildsystem.name()
                );
            }
            Err(e) => return Err(e),
        }
    }
    Ok(())
}

/// Install the source package's Debian Build-Depends (from debian/control) via
/// apt, shared by the `scip` and `exec` `--apt-build-deps` options.
fn install_apt_build_deps(
    #[allow(unused_variables)] session: &dyn Session,
    #[allow(unused_variables)] external_dir: &Path,
) -> Result<(), Error> {
    #[cfg(feature = "debian")]
    {
        let control = external_dir.join("debian/control");
        if control.exists() {
            log::info!("Installing Debian Build-Depends from {}", control.display());
            ognibuild::debian::satisfy_build_deps_from_control(session, &control)
                .map_err(|e| Error::Other(e.to_string()))?;
        } else {
            log::info!("--apt-build-deps given but no debian/control found; skipping");
        }
        Ok(())
    }
    #[cfg(not(feature = "debian"))]
    Err(Error::Other(
        "--apt-build-deps requires ogni built with the 'debian' feature".to_string(),
    ))
}

/// Copy a file produced inside the session out to the host, given a
/// "SESSION_PATH:HOST_PATH" spec. A relative SESSION_PATH is resolved against
/// the session's working directory.
fn copy_out_of_session(session: &dyn Session, spec: &str) -> Result<(), Error> {
    let (session_path, host_path) = spec.split_once(':').ok_or_else(|| {
        Error::Other(format!(
            "--copy-out expects SESSION_PATH:HOST_PATH, got {:?}",
            spec
        ))
    })?;
    let session_path = Path::new(session_path);
    let in_session = if session_path.is_absolute() {
        session_path.to_path_buf()
    } else {
        session.pwd().join(session_path)
    };
    let src = session.external_path(&in_session);
    std::fs::copy(&src, host_path).map_err(|e| {
        Error::Other(format!(
            "Failed to copy {} out of the session to {}: {}",
            src.display(),
            host_path,
            e
        ))
    })?;
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
    if let Some(Command::Exec(ExecArgs {
        subargv,
        apt_build_deps,
        copy_out,
    })) = &args.command
    {
        if *apt_build_deps {
            install_apt_build_deps(session, external_dir)?;
        }
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
        for spec in copy_out {
            copy_out_of_session(session, spec)?;
        }
        return Ok(());
    }
    let mut log_manager = ognibuild::logs::NoLogManager;
    let bss = detect_buildsystems(external_dir);
    // "scip --apt-build-deps" installs the Debian source package's Build-Depends
    // from debian/control, which are then the authoritative set. Installing the
    // build systems' own declared dependencies on top is redundant and can fail
    // for reasons irrelevant to indexing (e.g. a JS build system pulling in
    // puppeteer, which downloads a browser), so skip that step in this case.
    let apt_build_deps = matches!(
        args.command.as_ref(),
        Some(Command::Scip(scip_args)) if scip_args.apt_build_deps
    );
    if !args.ignore_declared_dependencies && !apt_build_deps {
        let categories = match args.command.as_ref().unwrap() {
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
            Command::CacheEnv(_) => return Ok(()), // No dependencies needed
            Command::Scip(_) => vec![DependencyCategory::Universal, DependencyCategory::Build],
            Command::Lsif(_) => vec![DependencyCategory::Universal, DependencyCategory::Build],
        };
        if !categories.is_empty() {
            log::info!("Checking that declared dependencies are present");
            if !args.explain {
                match install_necessary_declared_dependencies(
                    session,
                    installer,
                    &[scope, InstallationScope::Vendor],
                    fixers,
                    bss.iter()
                        .map(|bs| bs.as_ref())
                        .collect::<Vec<_>>()
                        .as_slice(),
                    &categories,
                ) {
                    Ok(_) => {}
                    Err(e) => {
                        log::info!("Unable to install declared dependencies: {}", e);
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

    match args.command.as_ref().unwrap() {
        Command::Exec(..) => unreachable!(),
        Command::CacheEnv(..) => unreachable!(),
        Command::Scip(scip_args) => {
            if scip_args.apt_build_deps {
                install_apt_build_deps(session, external_dir)?;
            }
            let bss = bss.iter().map(|bs| bs.as_ref()).collect::<Vec<_>>();
            if let Some(output_dir) = scip_args.output_all.as_ref() {
                ognibuild::actions::scip::run_scip_multi(
                    session,
                    bss.as_slice(),
                    installer,
                    fixers,
                    output_dir.as_path(),
                )?;
            } else {
                let output = scip_args
                    .output
                    .clone()
                    .unwrap_or_else(|| PathBuf::from("index.scip"));
                ognibuild::actions::scip::run_scip(
                    session,
                    bss.as_slice(),
                    installer,
                    fixers,
                    output.as_path(),
                )?;
            }
        }
        Command::Lsif(lsif_args) => {
            ognibuild::actions::lsif::run_lsif(
                session,
                bss.iter()
                    .map(|bs| bs.as_ref())
                    .collect::<Vec<_>>()
                    .as_slice(),
                installer,
                fixers,
                lsif_args.output.as_path(),
            )?;
        }
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

    if args.supported_buildsystems {
        for bs in supported_buildsystem_names() {
            println!("{}", bs);
        }
        return Ok(());
    }

    // Check if command is provided
    let command = match args.command {
        Some(cmd) => cmd,
        None => {
            eprintln!("Error: No command provided");
            return Err(1);
        }
    };
    args.command = Some(command);

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

    // Handle cache-env command separately as it doesn't need a session
    if let Some(Command::CacheEnv(ref cache_args)) = args.command {
        let include = cache_args
            .include
            .iter()
            .map(|s| s.as_str())
            .collect::<Vec<_>>();
        return cache_debian_image(
            &cache_args.suite,
            cache_args.force,
            cache_args.update,
            &include,
        );
    }

    #[cfg(target_os = "linux")]
    let schroot = args.schroot.clone();
    #[cfg(not(target_os = "linux"))]
    let schroot = None;
    let session_kind = match ognibuild::session::resolve_session_kind(args.session.clone(), schroot)
    {
        Ok(kind) => kind,
        Err(e) => {
            eprintln!("Error: {}", e);
            return Err(1);
        }
    };
    let mut session: Box<dyn Session> = session_kind.build(None).map_err(|e| {
        eprintln!("Error: Failed to set up session: {}", e);
        1
    })?;

    // Indexing needs the network (to install build dependencies), so allow it
    // unless explicitly asked to stay offline. Only sessions that isolate the
    // network (unshare) act on this.
    if let Some(Command::Scip(scip_args)) = args.command.as_ref() {
        session.set_isolate_network(scip_args.offline);
    }

    #[cfg(feature = "breezy")]
    if let Err(e) = breezyshim::try_init() {
        log::error!("Unable to initialize Breezy: {}", e);
        return Err(1);
    }

    #[cfg(feature = "breezy")]
    let url = if let Ok(url) = args.directory.parse::<url::Url>() {
        url
    } else {
        let p = Path::new(&args.directory);
        let abs = p.canonicalize().map_err(|e| {
            eprintln!("Error: Cannot access directory {}: {}", args.directory, e);
            1
        })?;
        url::Url::from_directory_path(&abs).map_err(|()| {
            eprintln!("Error: Invalid directory path: {}", abs.display());
            1
        })?
    };
    #[cfg(feature = "breezy")]
    let mut td: Option<tempfile::TempDir> = None;
    // TODO(jelmer): Get a list of supported schemes from breezy?
    #[cfg(feature = "breezy")]
    let project = if ["git", "http", "https", "ssh"].contains(&url.scheme()) {
        let b = breezyshim::branch::open_as_generic(&url).unwrap();
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
        session
            .project_from_directory(&directory, None)
            .map_err(|e| {
                eprintln!(
                    "Error: Failed to prepare directory {}: {}",
                    directory.display(),
                    e
                );
                1
            })?
    };

    #[cfg(not(feature = "breezy"))]
    let project = {
        let directory = PathBuf::from(args.directory.clone());
        log::info!("Preparing directory {}", directory.display());
        session
            .project_from_directory(&directory, None)
            .map_err(|e| {
                eprintln!(
                    "Error: Failed to prepare directory {}: {}",
                    directory.display(),
                    e
                );
                1
            })?
    };

    session.chdir(project.internal_path()).unwrap();
    std::env::set_current_dir(project.external_path()).unwrap();

    if !session.is_temporary() && matches!(args.command, Some(Command::Info)) {
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
    } else if args.installer.contains(&"apt".to_string()) {
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

    #[cfg(feature = "breezy")]
    std::mem::drop(td);
    Ok(())
}

#[cfg(target_os = "linux")]
fn cache_debian_image(suite: &str, force: bool, update: bool, include: &[&str]) -> Result<(), i32> {
    if is_network_disabled() {
        eprintln!("Error: Network access is disabled (OGNIBUILD_DISABLE_NET is set)");
        eprintln!("Cannot download or update a Debian image without network access.");
        return Err(1);
    }

    let arch = std::env::consts::ARCH;
    let arch_name = match arch {
        "x86_64" => "amd64",
        "aarch64" => "arm64",
        _ => {
            eprintln!("Unsupported architecture: {}", arch);
            return Err(1);
        }
    };

    let cache_dir = match dirs::cache_dir() {
        Some(dir) => dir.join("ognibuild").join("images"),
        None => {
            eprintln!("Cannot determine cache directory");
            return Err(1);
        }
    };

    if let Err(e) = std::fs::create_dir_all(&cache_dir) {
        eprintln!("Failed to create cache directory: {}", e);
        return Err(1);
    }

    let tarball_name = format!("debian-{}-{}.tar.gz", suite, arch_name);
    let tarball_path = cache_dir.join(&tarball_name);

    if update {
        if !tarball_path.exists() {
            eprintln!(
                "No cached Debian {} image at {} to update.",
                suite,
                tarball_path.display()
            );
            eprintln!("Run 'ogni cache-env {}' to create one first.", suite);
            return Err(1);
        }
        return update_debian_image(suite, &tarball_path);
    }

    if tarball_path.exists() && !force {
        log::info!(
            "Debian {} image already cached at {}",
            suite,
            tarball_path.display()
        );
        log::info!("Use --force to re-download, or --update to apt upgrade in place.");
        return Ok(());
    }

    // Bootstrap a Debian session using mmdebstrap and save it
    log::info!("Bootstrapping Debian {} image using mmdebstrap...", suite);
    let session = match ognibuild::session::unshare::bootstrap_debian_tarball(suite, true, include)
    {
        Ok(session) => session,
        Err(e) => {
            eprintln!("Failed to bootstrap image: {}", e);
            return Err(1);
        }
    };

    save_cached_image(suite, &session, &tarball_path)
}

#[cfg(target_os = "linux")]
fn update_debian_image(suite: &str, tarball_path: &Path) -> Result<(), i32> {
    use ognibuild::session::unshare::UnshareSession;

    log::info!("Loading cached Debian {} image for update...", suite);
    let session = match UnshareSession::from_tarball(tarball_path) {
        Ok(session) => session,
        Err(e) => {
            eprintln!("Failed to load cached image: {}", e);
            return Err(1);
        }
    };

    // apt needs network access to reach the Debian mirrors.
    session.set_isolate_network(false);

    let mut env = std::collections::HashMap::new();
    env.insert("DEBIAN_FRONTEND".to_string(), "noninteractive".to_string());

    // Error-Mode=any turns a failed index fetch into a non-zero exit, since
    // apt-get update otherwise reports success and silently falls back to the
    // stale lists.
    log::info!("Running apt-get update...");
    if let Err(e) = session.check_call(
        vec!["apt-get", "-o", "APT::Update::Error-Mode=any", "update"],
        Some(Path::new("/")),
        Some("root"),
        Some(env.clone()),
    ) {
        eprintln!("apt-get update failed: {}", e);
        return Err(1);
    }

    log::info!("Running apt-get full-upgrade...");
    if let Err(e) = session.check_call(
        vec!["apt-get", "full-upgrade", "--yes"],
        Some(Path::new("/")),
        Some("root"),
        Some(env),
    ) {
        eprintln!("apt-get full-upgrade failed: {}", e);
        return Err(1);
    }

    save_cached_image(suite, &session, tarball_path)
}

#[cfg(target_os = "linux")]
fn save_cached_image(
    suite: &str,
    session: &ognibuild::session::unshare::UnshareSession,
    tarball_path: &Path,
) -> Result<(), i32> {
    log::info!("Saving to cache: {}", tarball_path.display());
    match session.save_to_tarball(tarball_path) {
        Ok(_) => {
            log::info!(
                "Successfully cached Debian {} image at {}",
                suite,
                tarball_path.display()
            );
            log::info!("");
            log::info!("This cached image will now be used automatically by tests.");
            log::info!("You can also explicitly use it by setting:");
            log::info!("  OGNIBUILD_DEBIAN_TEST_TARBALL={}", tarball_path.display());
            Ok(())
        }
        Err(e) => {
            eprintln!("Failed to save tarball: {}", e);
            Err(1)
        }
    }
}

#[cfg(not(target_os = "linux"))]
fn cache_debian_image(
    _suite: &str,
    _force: bool,
    _update: bool,
    _include: &[&str],
) -> Result<(), i32> {
    eprintln!("Error: cache-env command is only available on Linux");
    Err(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_is_network_disabled_not_set() {
        use std::collections::HashMap;
        let env: HashMap<String, String> = HashMap::new();
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));
    }

    #[test]
    fn test_is_network_disabled_true() {
        use std::collections::HashMap;

        let mut env = HashMap::new();
        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "1".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "true".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "TRUE".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "yes".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "YES".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "on".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "ON".to_string());
        assert!(is_network_disabled_with(|key| env.get(key).cloned()));
    }

    #[test]
    fn test_is_network_disabled_false() {
        use std::collections::HashMap;

        let mut env = HashMap::new();
        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "0".to_string());
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "false".to_string());
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "no".to_string());
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "off".to_string());
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));

        env.insert("OGNIBUILD_DISABLE_NET".to_string(), "random".to_string());
        assert!(!is_network_disabled_with(|key| env.get(key).cloned()));
    }
}
