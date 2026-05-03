//! Resource URI encoding/decoding for gateway aggregation (#732).
//!
//! Backend DCC servers expose resource URIs like `scene://current` or
//! `capture://current_window`. The gateway aggregates resources from
//! multiple backends, so it prefixes each backend URI with the 8-hex-char
//! instance id to disambiguate:
//!
//!   `scene://current` on backend `abcdef01...`  →  `scene://abcdef01/current`
//!
//! The instance-id segment is the first path segment following `<scheme>://`.
//! When the first segment is a valid 8-char hex string, the URI is treated
//! as gateway-encoded and decoded back to the backend's original URI.
//!
//! Backend URIs that happen to start with 8 hex chars (unlikely in practice —
//! `scene://deadbeef` would be a path, not a prefix) are unambiguously
//! handled: the gateway only emits the prefixed form, and backends never
//! see the prefixed form on `resources/read`.
//!
//! Unlike the tool-name encoding, resource URIs already use `://` which is
//! not in the tool-name alphabet, so no escape layer is required — the
//! `<scheme>://<id8>/<rest>` shape is preserved byte-for-byte.

use uuid::Uuid;

use super::constants::{ID_PREFIX_LEN, instance_short};

/// Encode a backend resource URI for gateway aggregation:
/// `<scheme>://<rest>` → `<scheme>://<id8>/<rest>`.
///
/// Preserves the scheme. When the backend URI has an empty authority
/// (e.g. `scene://current`), the instance id becomes the authority and
/// the original path is appended after `/`. When the backend URI already
/// has an authority (e.g. `file://host/path`), the id is inserted before
/// the authority so the authority becomes part of the path.
///
/// Returns `None` when the input does not contain `://`.
pub fn encode_resource_uri(id: &Uuid, backend_uri: &str) -> Option<String> {
    let (scheme, rest) = backend_uri.split_once("://")?;
    let id8 = instance_short(id);
    // Normalise: drop any leading `/` from the rest so we emit exactly one.
    let rest = rest.trim_start_matches('/');
    Some(format!("{scheme}://{id8}/{rest}"))
}

/// Decode a gateway-prefixed resource URI into `(id8, backend_uri)`.
///
/// Returns `None` when the URI does not follow the `<scheme>://<id8>/<rest>`
/// shape — callers fall back to the legacy `dcc://<type>/<id>` admin pointer
/// handling.
///
/// The returned `backend_uri` always reconstructs the scheme, so callers can
/// forward it directly to the owning backend's `resources/read`.
pub fn decode_resource_uri(prefixed: &str) -> Option<(String, String)> {
    let (scheme, rest) = prefixed.split_once("://")?;
    // Extract the first path segment — that's the instance-id candidate.
    let (id_candidate, remainder) = match rest.split_once('/') {
        Some((head, tail)) => (head, tail),
        // No trailing slash at all: the whole rest is the id candidate and
        // the backend URI is `<scheme>://` (empty rest).
        None => (rest, ""),
    };
    if !is_instance_id8(id_candidate) {
        return None;
    }
    let backend_uri = format!("{scheme}://{remainder}");
    Some((id_candidate.to_string(), backend_uri))
}

/// Return `true` iff `s` is an 8-char lowercase hex string — the canonical
/// shape emitted by [`instance_short`].
fn is_instance_id8(s: &str) -> bool {
    s.len() == ID_PREFIX_LEN && s.bytes().all(|b| b.is_ascii_hexdigit())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn fixed_id() -> Uuid {
        Uuid::parse_str("abcdef01-0000-0000-0000-000000000000").unwrap()
    }

    #[test]
    fn encodes_simple_scheme_uri() {
        let id = fixed_id();
        let encoded = encode_resource_uri(&id, "scene://current").unwrap();
        assert_eq!(encoded, "scene://abcdef01/current");
    }

    #[test]
    fn encodes_uri_with_path_components() {
        let id = fixed_id();
        let encoded = encode_resource_uri(&id, "output://job_42/stdout.log").unwrap();
        assert_eq!(encoded, "output://abcdef01/job_42/stdout.log");
    }

    #[test]
    fn encodes_uri_whose_rest_starts_with_slash() {
        // Some backends emit `scene:///current` (triple slash). Normalise
        // so the prefixed form has exactly one `/` after the id.
        let id = fixed_id();
        let encoded = encode_resource_uri(&id, "scene:///current").unwrap();
        assert_eq!(encoded, "scene://abcdef01/current");
    }

    #[test]
    fn encode_rejects_non_uri() {
        assert!(encode_resource_uri(&fixed_id(), "not-a-uri").is_none());
    }

    #[test]
    fn decodes_prefixed_uri_round_trip() {
        let id = fixed_id();
        for backend_uri in &[
            "scene://current",
            "capture://current_window",
            "output://job_42/stdout.log",
            "artefact://session/42/meta.json",
        ] {
            let prefixed = encode_resource_uri(&id, backend_uri).unwrap();
            let (id8, decoded) = decode_resource_uri(&prefixed).unwrap();
            assert_eq!(id8, "abcdef01");
            // Normalise the expected backend uri: encoder strips leading
            // slashes from the rest, so triple-slash input comes back as
            // single-slash. The gateway forwards `decoded` to the backend,
            // and MCP spec treats `scheme://x` and `scheme:///x` as
            // equivalent so either is valid; our contract is "whatever
            // round-trips exactly".
            let expected = {
                let (scheme, rest) = backend_uri.split_once("://").unwrap();
                format!("{scheme}://{}", rest.trim_start_matches('/'))
            };
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn decode_rejects_uris_without_valid_id_prefix() {
        // Admin pointer: first segment is `maya`, not an 8-hex id.
        assert!(decode_resource_uri("dcc://maya/abc").is_none());
        // Too short.
        assert!(decode_resource_uri("scene://abc/current").is_none());
        // Too long.
        assert!(decode_resource_uri("scene://abcdef0123/current").is_none());
        // Non-hex.
        assert!(decode_resource_uri("scene://xyzxyzxy/current").is_none());
        // No scheme separator.
        assert!(decode_resource_uri("abcdef01/current").is_none());
    }

    #[test]
    fn decode_handles_empty_backend_path() {
        // `scene://abcdef01` (no trailing slash, no rest) maps to the
        // backend URI `scene://` which is unusual but well-formed per RFC.
        let (id8, backend) = decode_resource_uri("scene://abcdef01").unwrap();
        assert_eq!(id8, "abcdef01");
        assert_eq!(backend, "scene://");
    }
}
