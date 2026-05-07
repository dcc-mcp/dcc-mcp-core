//! Shared parsing helpers for `gateway://*` URIs.

use std::collections::HashMap;

/// Parse a URI query string into a flat `key → value` map.
///
/// `key` (no `=`) is treated as `key=true` for compatibility with bare
/// flag-style query parameters.
///
/// Unknown parameters are *not* filtered here — callers decide what to do
/// with them (typically: ignore for forward compatibility).
pub fn parse_query(query: &str) -> HashMap<&str, &str> {
    let mut out = HashMap::new();
    for pair in query.split('&').filter(|p| !p.is_empty()) {
        let (k, v) = pair.split_once('=').unwrap_or((pair, "true"));
        out.insert(k, v);
    }
    out
}

/// Liberal boolean parser used across `gateway://*` query params.
///
/// Accepts `true`/`1`/`yes`/`on` and their inverses, all case-insensitively.
/// Returns `None` (rather than a default) so callers can preserve their own
/// default for unrecognised values.
pub fn parse_bool(s: &str) -> Option<bool> {
    match s.trim().to_ascii_lowercase().as_str() {
        "true" | "1" | "yes" | "on" => Some(true),
        "false" | "0" | "no" | "off" => Some(false),
        _ => None,
    }
}

/// Split a URI into `(path, query)` where `query` is `None` when no `?` is
/// present. Both halves are returned as borrowed slices of the input.
pub fn split_uri(uri: &str) -> (&str, Option<&str>) {
    match uri.split_once('?') {
        Some((p, q)) => (p, Some(q)),
        None => (uri, None),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_query_handles_empty() {
        assert!(parse_query("").is_empty());
    }

    #[test]
    fn parse_query_handles_bare_flag() {
        let m = parse_query("verbose");
        assert_eq!(m.get("verbose"), Some(&"true"));
    }

    #[test]
    fn parse_query_handles_pairs() {
        let m = parse_query("a=1&b=2");
        assert_eq!(m.get("a"), Some(&"1"));
        assert_eq!(m.get("b"), Some(&"2"));
    }

    #[test]
    fn parse_bool_accepts_truthy() {
        for s in ["true", "TRUE", "1", "yes", "ON"] {
            assert_eq!(parse_bool(s), Some(true), "parsing {s}");
        }
    }

    #[test]
    fn parse_bool_accepts_falsy() {
        for s in ["false", "FALSE", "0", "no", "OFF"] {
            assert_eq!(parse_bool(s), Some(false), "parsing {s}");
        }
    }

    #[test]
    fn parse_bool_rejects_unknown() {
        for s in ["maybe", "", "2"] {
            assert_eq!(parse_bool(s), None, "parsing {s}");
        }
    }

    #[test]
    fn split_uri_handles_no_query() {
        assert_eq!(split_uri("foo"), ("foo", None));
    }

    #[test]
    fn split_uri_handles_query() {
        assert_eq!(split_uri("foo?bar=1"), ("foo", Some("bar=1")));
    }
}
