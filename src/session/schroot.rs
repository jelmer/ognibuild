extern crate rand;
use rand::Rng;
use std::iter;

pub fn sanitize_session_name(name: &str) -> String {
    name.chars()
        .filter(|&c| c.is_alphanumeric() || "_-.".contains(c))
        .collect()
}

pub fn generate_session_id(prefix: &str) -> String {
    let suffix: String = String::from_utf8(
        iter::repeat(())
            .map(|()| rand::thread_rng().sample(rand::distributions::Alphanumeric))
            .take(8)
            .collect(),
    )
    .unwrap();
    format!("{}-{}", sanitize_session_name(prefix), suffix)
}
