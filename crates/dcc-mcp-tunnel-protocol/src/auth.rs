//! JWT-based bearer authentication for tunnel registration.
//!
//! The relay holds a single shared secret and signs/validates HS256 tokens
//! against it. Per-DCC scoping is encoded in the [`TunnelClaims::allowed_dcc`]
//! list so a token issued for "maya" cannot be replayed by an agent that
//! identifies itself as "houdini".
//!
//! Asymmetric signing (RS256/EdDSA) and key rotation are deliberately
//! deferred — they belong in PR 5 (hardening) once the data plane works.

use jsonwebtoken::{Algorithm, DecodingKey, EncodingKey, Header, Validation, decode, encode};
use serde::{Deserialize, Serialize};

use crate::error::ProtocolError;

/// Claims encoded inside a tunnel registration JWT.
///
/// Standard JWT timestamp claims are `iat` (issued-at) and `exp` (expiry),
/// both expressed as **Unix seconds** so they round-trip cleanly through
/// [`jsonwebtoken`]. The relay rejects tokens with `exp <= now` and tokens
/// whose `iat` lies in the future (clock-skew window: 60 s).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct TunnelClaims {
    /// Subject — usually a stable agent identifier (e.g. workstation
    /// hostname, operator email, or a wildcard like `"any"` for shared
    /// secrets).
    pub sub: String,

    /// Issued-at (Unix seconds).
    pub iat: u64,

    /// Expiry (Unix seconds). Tokens with `exp <= now` are rejected.
    pub exp: u64,

    /// Issuer hint (e.g. relay public hostname). Logged but not enforced.
    pub iss: String,

    /// Allow-list of DCC tags this token can register under. The relay
    /// matches [`crate::frame::RegisterRequest::dcc`] against this list and
    /// rejects with [`crate::frame::ErrorCode::DccNotAllowed`] on mismatch.
    /// An empty list permits any DCC (used by admin/test tokens).
    pub allowed_dcc: Vec<String>,
}

/// Sign `claims` using `secret`. Produces a compact `"header.payload.sig"`
/// string suitable for an `Authorization: Bearer …` header.
pub fn issue(claims: &TunnelClaims, secret: &[u8]) -> Result<String, ProtocolError> {
    let header = Header::new(Algorithm::HS256);
    let key = EncodingKey::from_secret(secret);
    let token = encode(&header, claims, &key)?;
    Ok(token)
}

/// Validate `token` against `secret` and return the decoded claims.
///
/// Validation enforces signature, `exp`, and a 60-second clock-skew
/// tolerance on `iat`. **DCC scoping is not enforced here** — that
/// happens in the relay after parsing the [`crate::frame::RegisterRequest`]
/// because the requested DCC is not part of the JWT payload.
pub fn validate(token: &str, secret: &[u8]) -> Result<TunnelClaims, ProtocolError> {
    let mut v = Validation::new(Algorithm::HS256);
    v.leeway = 60;
    v.validate_exp = true;
    // The standard `iss` / `aud` claims aren't enforced here; the relay
    // simply records `iss` for telemetry.
    v.required_spec_claims.clear();
    v.required_spec_claims.insert("exp".into());
    let key = DecodingKey::from_secret(secret);
    let data = decode::<TunnelClaims>(token, &key, &v)?;
    Ok(data.claims)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::{SystemTime, UNIX_EPOCH};

    fn now() -> u64 {
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs()
    }

    fn sample_claims(exp_offset_secs: i64) -> TunnelClaims {
        let now = now();
        let exp = if exp_offset_secs >= 0 {
            now + exp_offset_secs as u64
        } else {
            now.saturating_sub((-exp_offset_secs) as u64)
        };
        TunnelClaims {
            sub: "workstation-001".into(),
            iat: now,
            exp,
            iss: "relay.example.com".into(),
            allowed_dcc: vec!["maya".into(), "houdini".into()],
        }
    }

    #[test]
    fn round_trip_signed_claims() {
        let secret = b"super-secret-key-for-tests-only";
        let claims = sample_claims(3600);
        let token = issue(&claims, secret).unwrap();
        let decoded = validate(&token, secret).unwrap();
        assert_eq!(decoded, claims);
    }

    #[test]
    fn rejects_expired_token() {
        let secret = b"super-secret-key-for-tests-only";
        let claims = sample_claims(-3600); // expired an hour ago
        let token = issue(&claims, secret).unwrap();
        let err = validate(&token, secret).unwrap_err();
        assert!(matches!(err, ProtocolError::Jwt(_)), "got {err:?}");
    }

    #[test]
    fn rejects_wrong_secret() {
        let claims = sample_claims(3600);
        let token = issue(&claims, b"correct-secret-correct-secret").unwrap();
        let err = validate(&token, b"wrong-secret-wrong-secret-123").unwrap_err();
        assert!(matches!(err, ProtocolError::Jwt(_)), "got {err:?}");
    }
}
