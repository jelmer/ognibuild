use crate::UpstreamOutput;

pub struct BinaryOutput {
    name: String,
}

impl BinaryOutput {
    pub fn new(name: &str) -> Self {
        BinaryOutput {
            name: name.to_string(),
        }
    }
}

impl UpstreamOutput for BinaryOutput {
    fn family() -> &'static str {
        "binary"
    }
}
