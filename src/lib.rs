#[cfg(feature = "debian")]
pub mod debian;
pub mod dist_catcher;
pub mod fix_build;
pub mod logs;
pub mod resolver;
pub mod session;
pub mod shebang;
pub mod vcs;

pub trait UpstreamOutput {
    fn family() -> &'static str;

    fn get_declared_dependencies(&self) -> Vec<String>;
}
