//! HMAC-SHA256 webhook validation.
//!
//! Uses the GitHub-style `X-Hub-Signature-256: sha256=<hex>` header
//! convention and a constant-time comparison via the `subtle` crate.

use hmac::{Hmac, KeyInit, Mac};
use sha2::Sha256;
use subtle::ConstantTimeEq;

/// Header name used for HMAC signatures. Matches the GitHub / X-Hub
/// convention so existing webhook senders work without reconfiguration.
pub const HMAC_HEADER: &str = "x-hub-signature-256";

type HmacSha256 = Hmac<Sha256>;

/// Compute the canonical `sha256=<hex>` header value for `body` under
/// the given `secret`.
#[must_use]
pub fn compute_signature(secret: &[u8], body: &[u8]) -> String {
    let mut mac =
        HmacSha256::new_from_slice(secret).expect("HMAC-SHA256 accepts keys of any length");
    mac.update(body);
    let tag = mac.finalize().into_bytes();
    format!("sha256={}", hex::encode(tag))
}

/// Verify a header value against the expected signature, using a
/// constant-time comparison.
///
/// Returns `true` when the header exists and matches the computed
/// signature. Case-insensitive on the `sha256=` prefix; otherwise
/// byte-for-byte.
#[must_use]
pub fn verify_hub_signature_256(secret: &[u8], body: &[u8], header_value: Option<&str>) -> bool {
    let Some(header) = header_value else {
        return false;
    };
    let expected = compute_signature(secret, body);
    constant_time_eq_ignore_prefix_case(&expected, header)
}

fn constant_time_eq_ignore_prefix_case(expected: &str, got: &str) -> bool {
    let (a_prefix, a_hex) = match expected.split_once('=') {
        Some(p) => p,
        None => return false,
    };
    let (b_prefix, b_hex) = match got.split_once('=') {
        Some(p) => p,
        None => return false,
    };
    if !a_prefix.eq_ignore_ascii_case(b_prefix) {
        return false;
    }
    let a_bytes = a_hex.as_bytes();
    let b_bytes = b_hex.as_bytes();
    if a_bytes.len() != b_bytes.len() {
        return false;
    }
    a_bytes.ct_eq(b_bytes).into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn matches_known_signature() {
        let secret = b"top-secret";
        let body = b"{\"hello\":\"world\"}";
        let sig = compute_signature(secret, body);
        assert!(verify_hub_signature_256(secret, body, Some(&sig)));
    }

    #[test]
    fn rejects_tampered_body() {
        let secret = b"top-secret";
        let sig = compute_signature(secret, b"a");
        assert!(!verify_hub_signature_256(secret, b"b", Some(&sig)));
    }

    #[test]
    fn rejects_missing_header() {
        assert!(!verify_hub_signature_256(b"x", b"a", None));
    }

    #[test]
    fn rejects_missing_prefix() {
        let secret = b"top-secret";
        let tag = compute_signature(secret, b"a");
        let hex_only = tag.trim_start_matches("sha256=");
        assert!(!verify_hub_signature_256(secret, b"a", Some(hex_only)));
    }

    #[test]
    fn accepts_case_insensitive_prefix() {
        let secret = b"top-secret";
        let sig = compute_signature(secret, b"a");
        let upper = sig.replacen("sha256", "SHA256", 1);
        assert!(verify_hub_signature_256(secret, b"a", Some(&upper)));
    }
}
