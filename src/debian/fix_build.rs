use breezyshim::commit::CommitReporter;
use breezyshim::debian::debcommit::debcommit;
use breezyshim::error::Error as BrzError;
use breezyshim::tree::{MutableTree, Tree};
use breezyshim::workingtree::WorkingTree;
use std::path::{Path, PathBuf};

pub fn rescue_build_log(
    output_directory: &Path,
    tree: Option<&WorkingTree>,
) -> Result<(), std::io::Error> {
    let xdg_cache_dir = std::env::var("XDG_CACHE_HOME").ok().map_or_else(
        || std::env::home_dir().unwrap().join(".cache"),
        PathBuf::from,
    );
    let buildlogs_dir = xdg_cache_dir.join("ognibuild/buildlogs");
    std::fs::create_dir_all(&buildlogs_dir)?;

    let target_log_file = buildlogs_dir.join(format!(
        "{}-{}.log",
        tree.map_or_else(|| PathBuf::from("build"), |t| t.basedir())
            .display(),
        chrono::Local::now().format("%Y-%m-%d_%H%M%s"),
    ));
    std::fs::copy(output_directory.join("build.log"), &target_log_file)?;
    log::info!("Build log available in {}", target_log_file.display());

    Ok(())
}

pub struct DebianPackagingContext {
    tree: WorkingTree,
    subpath: PathBuf,
    committer: (String, String),
    update_changelog: bool,
    commit_reporter: Box<dyn CommitReporter>,
}

impl DebianPackagingContext {
    pub fn new(
        tree: WorkingTree,
        subpath: PathBuf,
        committer: Option<(String, String)>,
        update_changelog: bool,
        commit_reporter: Box<dyn CommitReporter>,
    ) -> Self {
        Self {
            tree,
            subpath,
            committer: committer.unwrap_or_else(|| debian_changelog::get_maintainer().unwrap()),
            update_changelog,
            commit_reporter,
        }
    }

    pub fn abspath(&self, path: &Path) -> PathBuf {
        self.tree.abspath(&self.subpath.join(path)).unwrap()
    }

    pub fn commit(&self, summary: &str, update_changelog: Option<bool>) -> Result<bool, BrzError> {
        let update_changelog = update_changelog.unwrap_or(self.update_changelog);

        let committer = format!("{} <{}>", self.committer.0, self.committer.1);

        let lock_write = self.tree.lock_write();
        let r = if update_changelog {
            let cl_path = self.abspath(Path::new("debian/changelog"));
            let mut f = self.tree.get_file(&cl_path).unwrap();
            let mut cl = debian_changelog::ChangeLog::read_relaxed(&mut f).unwrap();
            cl.auto_add_change(&[summary], self.committer.clone(), None, None);

            debcommit(
                &self.tree,
                Some(&committer),
                &self.subpath,
                None,
                Some(self.commit_reporter.as_ref()),
                None,
            )
        } else {
            self.tree
                .build_commit()
                .message(summary)
                .committer(&committer)
                .specific_files(&[&self.subpath])
                .reporter(self.commit_reporter.as_ref())
                .commit()
        };

        std::mem::drop(lock_write);

        match r {
            Ok(_) => Ok(true),
            Err(BrzError::PointlessCommit) => Ok(false),
            Err(e) => {
                return Err(e);
            }
        }
    }
}
