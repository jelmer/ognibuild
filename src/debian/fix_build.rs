use std::path::{PathBuf,Path};
use breezyshim::workingtree::WorkingTree;

pub fn rescue_build_log(output_directory: &Path, tree: Option<&WorkingTree>) -> Result<(), std::io::Error> {
    let xdg_cache_dir = std::env::var("XDG_CACHE_HOME").ok().map_or_else(|| std::env::home_dir().unwrap().join(".cache"), PathBuf::from);
    let buildlogs_dir = xdg_cache_dir.join("ognibuild/buildlogs");
    std::fs::create_dir_all(&buildlogs_dir)?;

    let target_log_file = buildlogs_dir.join(
        format!("{}-{}.log",
            tree.map_or_else(|| PathBuf::from("build"), |t| t.basedir()).display(),
            chrono::Local::now().format("%Y-%m-%d_%H%M%s"),
        ),
    );
    std::fs::copy(output_directory.join("build.log"), &target_log_file)?;
    log::info!("Build log available in {}", target_log_file.display());

    Ok(())
}
