//! Base crate for the backend workspace member.
//!
//! This crate starts intentionally small in issue #1. Later issues add the
//! HTTP server, configuration, database access, and application services on top
//! of this workspace member.

/// Returns the backend workspace member name.
pub fn crate_name() -> &'static str {
    "zeroclaw-server"
}

#[cfg(test)]
mod tests {
    use super::crate_name;

    #[test]
    fn exposes_crate_name() {
        assert_eq!(crate_name(), "zeroclaw-server");
    }
}
