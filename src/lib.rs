pub fn sanitize_session_name(name: &str) -> String {
        name.chars()
                    .filter(|&c| c.is_alphanumeric() || "_-.".contains(c))
                            .collect()
}
