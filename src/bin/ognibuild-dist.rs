use breezyshim::error::Error as BrzError;
use breezyshim::tree::Tree;
use breezyshim::workingtree::{self, WorkingTree};
use clap::Parser;
#[cfg(feature = "debian")]
use debian_control::Control;
use std::io::Write;
use std::path::{Path, PathBuf};

// These imports are only used in Linux-specific code
#[cfg(target_os = "linux")]
use ognibuild::analyze::AnalyzedError;
#[cfg(target_os = "linux")]
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

#[derive(Debug)]
enum ExportError {
    Brz(BrzError),
    Io(std::io::Error),
}

impl From<BrzError> for ExportError {
    fn from(e: BrzError) -> Self {
        ExportError::Brz(e)
    }
}

impl From<std::io::Error> for ExportError {
    fn from(e: std::io::Error) -> Self {
        ExportError::Io(e)
    }
}

impl std::fmt::Display for ExportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExportError::Brz(e) => write!(f, "{}", e),
            ExportError::Io(e) => write!(f, "{}", e),
        }
    }
}

impl std::error::Error for ExportError {}

fn export_tarball(tree: &dyn Tree, subpath: &Path, target: &Path) -> Result<(), ExportError> {
    let chunks = tree.archive(
        "tgz",
        "dist.tar.gz",
        Some("dist"),
        Some(subpath),
        None,
        false,
    )?;
    let mut f = std::fs::File::create(target)?;
    for chunk in chunks {
        f.write_all(&chunk?)?;
    }
    Ok(())
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

    if let Err(e) = breezyshim::try_init() {
        log::error!("Unable to initialize Breezy: {}", e);
        return Err(1);
    }

    let (tree, subpath) = workingtree::open_containing(&args.directory).unwrap();

    // These variables are only used in the Linux-specific code path
    #[cfg(all(target_os = "linux", feature = "debian"))]
    let (packaging_tree, packaging_subdir, package_name): (
        Option<Box<dyn WorkingTree>>,
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
            Some(Box::new(packaging_tree)),
            Some(packaging_subpath),
            Some(package_name),
        )
    } else {
        (None, None, None)
    };

    #[cfg(all(target_os = "linux", not(feature = "debian")))]
    let (packaging_tree, packaging_subdir, package_name): (
        Option<Box<dyn WorkingTree>>,
        Option<PathBuf>,
        Option<String>,
    ) = (None, None, None);

    match args.mode {
        Mode::Vcs => {
            export_tarball(&tree, &subpath, Path::new("dist.tar.gz")).unwrap();
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
                packaging_tree.as_ref().map(|t| &**t as &dyn Tree),
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
                        export_tarball(&tree, &subpath, Path::new("dist.tar.gz")).unwrap();
                        Ok(())
                    }
                }
                Err(Error::Unimplemented) => {
                    if args.mode == Mode::Buildsystem {
                        log::error!("Unable to ask buildsystem for tarball");
                        Err(1)
                    } else {
                        log::info!("Build system does not support dist tarball creation, falling back to simple export.");
                        export_tarball(&tree, &subpath, Path::new("dist.tar.gz")).unwrap();
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

#[cfg(test)]
mod tests {
    use super::*;
    use breezyshim::tree::MutableTree;

    fn tarball_names(path: &Path) -> Vec<String> {
        let f = std::fs::File::open(path).unwrap();
        let mut archive = tar::Archive::new(flate2::read::GzDecoder::new(f));
        archive
            .entries()
            .unwrap()
            .map(|e| e.unwrap().path().unwrap().display().to_string())
            .collect()
    }

    #[test]
    fn test_export_tarball() {
        let td = tempfile::tempdir().unwrap();
        let tree = breezyshim::controldir::create_standalone_workingtree(
            &td.path().join("tree"),
            &breezyshim::controldir::ControlDirFormat::default(),
        )
        .unwrap();
        tree.mkdir(Path::new("sub")).unwrap();
        tree.put_file_bytes_non_atomic(Path::new("README"), b"readme")
            .unwrap();
        tree.put_file_bytes_non_atomic(Path::new("sub/a.txt"), b"a")
            .unwrap();
        tree.add(&[Path::new("README"), Path::new("sub/a.txt")])
            .unwrap();

        let target = td.path().join("dist.tar.gz");
        export_tarball(&tree, Path::new(""), &target).unwrap();
        assert_eq!(
            tarball_names(&target),
            vec!["dist/README", "dist/sub/", "dist/sub/a.txt"]
        );

        export_tarball(&tree, Path::new("sub"), &target).unwrap();
        assert_eq!(tarball_names(&target), vec!["dist/a.txt"]);
    }
}
