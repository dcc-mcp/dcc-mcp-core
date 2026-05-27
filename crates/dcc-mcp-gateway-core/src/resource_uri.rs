//! Pure resource URI encoding for gateway aggregation (#732, #845).
//!
//! Backend DCC servers expose resource URIs like `scene://current` or
//! `capture://current_window`. The gateway aggregates resources from multiple
//! backends, so it prefixes each backend URI with the 8-hex-char instance id to
//! disambiguate:
//!
//! `scene://current` on backend `abcdef01...` becomes
//! `scene://abcdef01/current`.
//!
//! This module is pure domain code: no HTTP, no registry access, and no gateway
//! runtime state.

use uuid::Uuid;

use crate::naming::{ID_PREFIX_LEN, instance_short};

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
#[must_use]
pub fn encode_resource_uri(id: &Uuid, backend_uri: &str) -> Option<String> {
    let (scheme, rest) = backend_uri.split_once("://")?;
    let id8 = instance_short(id);
    let rest = rest.trim_start_matches('/');
    Some(format!("{scheme}://{id8}/{rest}"))
}

/// Decode a gateway-prefixed resource URI into `(id8, backend_uri)`.
///
/// Returns `None` when the URI does not follow the `<scheme>://<id8>/<rest>`
/// shape — callers fall back to legacy admin-pointer handling.
///
/// The returned `backend_uri` always reconstructs the scheme, so callers can
/// forward it directly to the owning backend's `resources/read`.
#[must_use]
pub fn decode_resource_uri(prefixed: &str) -> Option<(String, String)> {
    let (scheme, rest) = prefixed.split_once("://")?;
    let (id_candidate, remainder) = match rest.split_once('/') {
        Some((head, tail)) => (head, tail),
        None => (rest, ""),
    };
    if !is_instance_id8(id_candidate) {
        return None;
    }
    let backend_uri = format!("{scheme}://{remainder}");
    Some((id_candidate.to_string(), backend_uri))
}

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
            let expected = {
                let (scheme, rest) = backend_uri.split_once("://").unwrap();
                format!("{scheme}://{}", rest.trim_start_matches('/'))
            };
            assert_eq!(decoded, expected);
        }
    }

    #[test]
    fn decode_rejects_uris_without_valid_id_prefix() {
        assert!(decode_resource_uri("dcc://maya/abc").is_none());
        assert!(decode_resource_uri("scene://abc/current").is_none());
        assert!(decode_resource_uri("scene://abcdef0123/current").is_none());
        assert!(decode_resource_uri("scene://xyzxyzxy/current").is_none());
        assert!(decode_resource_uri("abcdef01/current").is_none());
    }

    #[test]
    fn decode_handles_empty_backend_path() {
        let (id8, backend) = decode_resource_uri("scene://abcdef01").unwrap();
        assert_eq!(id8, "abcdef01");
        assert_eq!(backend, "scene://");
    }
}
