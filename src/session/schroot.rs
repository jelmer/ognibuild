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

#[cfg(test)]
mod tests {
    #[test]
    fn test_sanitize_session_name() {
        assert_eq!(super::sanitize_session_name("foo"), "foo");
        assert_eq!(super::sanitize_session_name("foo-bar"), "foo-bar");
        assert_eq!(super::sanitize_session_name("foo_bar"), "foo_bar");
        assert_eq!(super::sanitize_session_name("foo.bar"), "foo.bar");
        assert_eq!(super::sanitize_session_name("foo!bar"), "foobar");
        assert_eq!(super::sanitize_session_name("foo@bar"), "foobar");
    }

    #[test]
    fn test_generate_session_id() {
        let id = super::generate_session_id("foo");
        assert_eq!(id.len(), 12);
        assert_eq!(&id[..4], "foo-");
    }
}
