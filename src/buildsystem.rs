#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    // Build: necessary to build the package
    Build,
    // core: necessary to do anything with the package
    Core,
    // test: necessary to run the tests
    Test,
    // dev: necessary for development (e.g. linters, yacc)
    Dev,
}

impl Stage {
    pub fn all() -> &'static [Stage] {
        &[Stage::Build, Stage::Core, Stage::Test, Stage::Dev]
    }
}

impl std::fmt::Display for Stage {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            Stage::Build => write!(f, "build"),
            Stage::Core => write!(f, "core"),
            Stage::Test => write!(f, "test"),
            Stage::Dev => write!(f, "dev"),
        }
    }
}
