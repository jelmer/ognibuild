use breezyshim::export::export;
use breezyshim::tree::Tree;
use breezyshim::workingtree::{self, WorkingTree};
use clap::Parser;
#[cfg(feature = "debian")]
use debian_control::Control;
use std::path::{Path, PathBuf};
use ognibuild::analyze::{AnalyzedError};
use ognibuild::buildsystem::Error;

#[derive(Clone, Default, PartialEq, Eq)]
pub enum Mode {
    #[default]
    Auto,
    Vcs,
    Buildsystem,
}

impl std::str::FromStr for Mode {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s {
            "auto" => Ok(Mode::Auto),
            "vcs" => Ok(Mode::Vcs),
            "buildsystem" => Ok(Mode::Buildsystem),
            _ => Err(format!("Unknown mode: {}", s)),
        }
    }
}

impl std::fmt::Display for Mode {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Mode::Auto => write!(f, "auto"),
            Mode::Vcs => write!(f, "vcs"),
            Mode::Buildsystem => write!(f, "buildsystem"),
        }
    }
}

#[derive(Parser)]
struct Args {
    #[clap(short, long, default_value = "unstable-amd64-sbuild")]
    /// Name of chroot to use
    chroot: String,

    #[clap(default_value = ".")]
    /// Directory with upstream source.
    directory: PathBuf,

    #[clap(long)]
    /// Path to packaging directory.
    packaging_directory: Option<PathBuf>,

    #[clap(long, default_value = "..")]
    /// Target directory
    target_directory: PathBuf,

    #[clap(long)]
    /// Enable debug output.
    debug: bool,

    #[clap(long, default_value = "auto")]
    /// Mechanism to use to create buildsystem
    mode: Mode,

    #[clap(long)]
    /// Include control directory in tarball.
    include_controldir: bool,
}

pub fn main() -> Result<(), i32> {
    let args = Args::parse();
    env_logger::builder()
        .filter_level(if args.debug {
            log::LevelFilter::Debug
        } else {
            log::LevelFilter::Info
        })
        .init();

    let (tree, subpath) = workingtree::open_containing(&args.directory).unwrap();

    #[cfg(feature = "debian")]
    let (packaging_tree, packaging_subdir, package_name): (
        Option<WorkingTree>,
        Option<PathBuf>,
        Option<String>,
    ) = if let Some(packaging_directory) = &args.packaging_directory {
        let (packaging_tree, packaging_subpath) =
            workingtree::open_containing(packaging_directory).unwrap();
        let text = packaging_tree
            .get_file(Path::new("debian/control"))
            .unwrap();
        let control: Control = Control::read(text).unwrap();
        let package_name = control.source().unwrap().name().unwrap();
        (
            Some(packaging_tree),
            Some(packaging_subpath),
            Some(package_name),
        )
    } else {
        (None, None, None)
    };

    #[cfg(not(feature = "debian"))]
    let (packaging_tree, packaging_subdir): (
        Option<WorkingTree>,
        Option<PathBuf>,
        Option<String>,
    ) = (None, None, None);

    match args.mode {
        Mode::Vcs => {
            export(&tree, Path::new("dist.tar.gz"), Some(&subpath)).unwrap();
            Ok(())
        }
        Mode::Auto | Mode::Buildsystem => {
            #[cfg(not(target_os = "linux"))]
            {
                log::error!("Unsupported mode: {}", args.mode);
                Err(1)
            }
            #[cfg(target_os = "linux")]
            match ognibuild::dist::create_dist_schroot(
                &tree,
                &args.target_directory.canonicalize().unwrap(),
                &args.chroot,
                packaging_tree.as_ref().map(|t| t as &dyn Tree),
                packaging_subdir.as_deref(),
                Some(args.include_controldir),
                &subpath,
                &mut ognibuild::logs::NoLogManager,
                None,
                package_name.as_deref(),
            ) {
                Ok(ret) => {
                    log::info!("Created {}", ret.to_str().unwrap());
                    Ok(())
                }
                Err(Error::IoError(e)) => {
                    log::error!("IO error: {}", e);
                    Err(1)
                }
                Err(Error::DependencyInstallError(e)) => {
                    log::error!("Dependency install error: {}", e);
                    Err(1)
                }
                Err(Error::NoBuildSystemDetected) => {
                    if args.mode == Mode::Buildsystem {
                        log::error!("No build system detected, unable to create tarball");
                        Err(1)
                    } else {
                        log::info!("No build system detected, falling back to simple export.");
                        export(&tree, Path::new("dist.tar.gz"), Some(&subpath)).unwrap();
                        Ok(())
                    }
                }
                Err(Error::Unimplemented) => {
                    if args.mode == Mode::Buildsystem {
                        log::error!("Unable to ask buildsystem for tarball");
                        Err(1)
                    } else {
                        log::info!("Build system does not support dist tarball creation, falling back to simple export.");
                        export(&tree, Path::new("dist.tar.gz"), Some(&subpath)).unwrap();
                        Ok(())
                    }
                }
                Err(Error::Error(AnalyzedError::Unidentified { lines, .. })) => {
                    log::error!("Unidentified error: {:?}", lines);
                    Err(1)
                }
                Err(Error::Error(AnalyzedError::Detailed { error, .. })) => {
                    log::error!("Identified error during dist creation: {}", error);
                    Err(1)
                }
                Err(e) => {
                    log::error!("Error: {}", e);
                    Err(1)
                }
            }
        }
    }
}
