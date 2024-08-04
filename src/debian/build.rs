pub fn get_build_architecture() -> String {
    std::process::Command::new("dpkg-architecture")
        .arg("-qDEB_BUILD_ARCH")
        .output()
        .map(|output| {
            String::from_utf8(output.stdout)
                .unwrap()
                .trim()
                .to_string()
        })
        .unwrap()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_build_architecture() {
        let arch = get_build_architecture();
        assert!(!arch.is_empty() && arch.len() < 10);
    }
}
