pub mod apt;
pub mod build;
pub mod build_deps;
pub mod context;
pub mod dep_server;
pub mod file_search;
pub mod fix_build;
pub mod sources_list;
#[cfg(feature = "udd")]
pub mod udd;
use breezyshim::tree::{Path, Tree};

use crate::session::Session;

pub fn satisfy_build_deps(
    session: &dyn Session,
    tree: &dyn Tree,
    debian_path: &Path,
) -> Result<(), apt::Error> {
    let path = debian_path.join("control");

    let f = tree.get_file_text(&path).unwrap();

    let control: debian_control::Control = String::from_utf8(f).unwrap().parse().unwrap();

    let source = control.source().unwrap();

    let mut deps = vec![];

    for dep in source
        .build_depends()
        .iter()
        .chain(source.build_depends_indep().iter())
        .chain(source.build_depends_arch().iter())
    {
        deps.push(apt::SatisfyEntry::Required(dep.to_string()));
    }

    for dep in source
        .build_conflicts()
        .iter()
        .chain(source.build_conflicts_indep().iter())
        .chain(source.build_conflicts_arch().iter())
    {
        deps.push(apt::SatisfyEntry::Conflict(dep.to_string()));
    }

    let apt_mgr = apt::AptManager::new(session, None);
    apt_mgr.satisfy(deps)
}
