//! Pure UUID and alphabet helpers used by gateway slug encoding.
//!
//! These primitives are the smallest pieces of the gateway namespace
//! contract: they have no third-party dependency beyond `uuid` and
//! describe encoding rules — not transport. They live here so the
//! [`crate::capability`] module can build slugs without dragging in
//! the full `dcc-mcp-gateway::namespace` module (which also owns
//! parsers for legacy / cursor-safe encodings, instance-watcher
//! warning state, and resource URI helpers).
//!
//! The full gateway `namespace` module re-exports these symbols so the
//! historical `crate::gateway::namespace::{instance_short,
//! is_cursor_safe_alphabet}` paths keep working.

use uuid::Uuid;

/// Length of the truncated instance UUID prefix used in encoded
/// tool names (e.g. `maya.abcdef01.create_sphere`). 8 hex chars
/// give 32 bits of entropy — enough to disambiguate among the
/// dozens of instances a gateway will ever see live, while staying
/// short enough to stay readable in log lines and error messages.
pub const ID_PREFIX_LEN: usize = 8;

/// Truncate a UUID to its first [`ID_PREFIX_LEN`] hex chars — the
/// canonical short form used inside encoded gateway tool names.
#[must_use]
pub fn instance_short(id: &Uuid) -> String {
    let mut s = id.simple().to_string();
    s.truncate(ID_PREFIX_LEN);
    s
}

/// Return `true` iff every byte of `s` is in the Cursor-safe alphabet
/// `[A-Za-z0-9_]`. Used as a cheap guard in debug assertions and in
/// tests — the stricter regex some MCP clients enforce.
#[must_use]
pub fn is_cursor_safe_alphabet(s: &str) -> bool {
    !s.is_empty() && s.bytes().all(|b| b.is_ascii_alphanumeric() || b == b'_')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn instance_short_takes_first_eight_hex() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let s = instance_short(&id);
        assert_eq!(s, "abcdef01");
        assert_eq!(s.len(), ID_PREFIX_LEN);
    }

    #[test]
    fn instance_short_is_lowercase_hex() {
        let id = Uuid::parse_str("ABCDEF0123456789ABCDEF0123456789").unwrap();
        let s = instance_short(&id);
        assert!(
            s.bytes()
                .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        );
    }

    #[test]
    fn cursor_safe_alphabet_accepts_word_chars() {
        assert!(is_cursor_safe_alphabet("create_sphere"));
        assert!(is_cursor_safe_alphabet("Maya2024"));
        assert!(is_cursor_safe_alphabet("a"));
    }

    #[test]
    fn cursor_safe_alphabet_rejects_separators_and_empty() {
        assert!(!is_cursor_safe_alphabet(""));
        assert!(!is_cursor_safe_alphabet("foo.bar"));
        assert!(!is_cursor_safe_alphabet("foo-bar"));
        assert!(!is_cursor_safe_alphabet("foo bar"));
        // Non-ASCII letters are filtered out for cursor compat.
        assert!(!is_cursor_safe_alphabet("café"));
    }
}
