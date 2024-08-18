use crate::Requirement;
use std::path::Path;

pub struct BinaryRequirement {
    binary_name: String,
}

impl BinaryRequirement {
    pub fn new(binary_name: &str) -> Self {
        Self {
            binary_name: binary_name.to_string(),
        }
    }
}

impl Requirement for BinaryRequirement {
    fn family(&self) -> &'static str {
        "binary"
    }

    fn met(&self, session: &dyn crate::session::Session) -> bool {
        let mut p = session.popen(
            (&["which", &self.binary_name]).to_vec(),
            Some(Path::new("/")),
            None,
            Some(std::process::Stdio::null()),
            Some(std::process::Stdio::null()),
            Some(std::process::Stdio::null()),
            None,
        );
        p.wait().unwrap().success()
    }
}
