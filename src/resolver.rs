pub trait Dependency {
    fn family(&self) -> &'static str;

    fn met(&self, session: &dyn crate::session::Session) -> bool;
}
