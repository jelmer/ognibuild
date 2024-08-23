pub trait Output {
    fn family() -> &'static str;

    fn get_declared_dependencies(&self) -> Vec<String>;
}
