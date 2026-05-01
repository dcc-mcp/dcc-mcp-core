//! Pluggable authentication gate for the per-DCC REST surface (#660).
//!
//! The default deployment is **localhost-only** — we never require
//! credentials on a DCC process that is bound to 127.0.0.1 because the
//! process boundary already is the trust boundary. When an operator
//! binds to a non-loopback address they must explicitly install a
//! [`BearerTokenGate`] (or any [`AuthGate`] impl of their own).

use std::net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr};

use super::errors::{ServiceError, ServiceErrorKind};

/// Bag of fields every auth gate needs to authorise a request. Kept
/// deliberately narrow (no axum types leak in) so future transports
/// can reuse the same trait.
#[derive(Debug, Clone)]
pub struct AuthContext<'a> {
    /// Socket address of the connecting peer, if the transport exposed
    /// it. `None` when the axum extractor could not extract it (e.g.
    /// running behind a Unix socket test harness).
    pub peer: Option<SocketAddr>,
    /// Value of the `Authorization` header, if any.
    pub authorization: Option<&'a str>,
    /// Value of the `X-Request-Id` header, if any — useful for
    /// propagating audit-trail correlation ids.
    pub request_id: Option<&'a str>,
}

/// Identity resolved by the gate. Kept open-ended so custom gates can
/// attach roles, tenant ids, etc. without the HTTP layer caring.
#[derive(Debug, Clone, Default)]
pub struct Principal {
    /// Display name for audit records.
    pub subject: String,
    /// Free-form role list — enterprises usually wire this to RBAC.
    pub roles: Vec<String>,
}

/// Pluggable auth policy. Implementations must be cheap to clone
/// (usually wrapping an `Arc`) because the router stores one copy and
/// calls `authorize` on every request.
pub trait AuthGate: Send + Sync {
    /// Return a [`Principal`] when the request is allowed, or a
    /// [`ServiceError`] with `kind = Unauthorized` otherwise.
    fn authorize(&self, ctx: &AuthContext<'_>) -> Result<Principal, ServiceError>;
}

// ── Default: allow loopback only ─────────────────────────────────────

/// Default gate — allow requests originating from the loopback
/// interface (IPv4 `127.0.0.0/8` or IPv6 `::1`), reject everything
/// else.
///
/// Safe default for single-user DCC deployments where the process is
/// bound to `127.0.0.1` and the OS is the trust boundary.
#[derive(Debug, Clone, Default)]
pub struct AllowLocalhostGate;

impl AllowLocalhostGate {
    pub const fn new() -> Self {
        Self
    }
}

fn is_loopback_ip(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(v4) => v4.is_loopback() || v4 == Ipv4Addr::UNSPECIFIED,
        IpAddr::V6(v6) => v6.is_loopback() || v6 == Ipv6Addr::UNSPECIFIED,
    }
}

impl AuthGate for AllowLocalhostGate {
    fn authorize(&self, ctx: &AuthContext<'_>) -> Result<Principal, ServiceError> {
        match ctx.peer {
            // No peer extracted — treat as trusted (test harnesses,
            // Unix-socket transports). Production axum extracts peer
            // on every connection so this branch only triggers in
            // tests.
            None => Ok(Principal {
                subject: "local".into(),
                roles: vec!["local".into()],
            }),
            Some(addr) if is_loopback_ip(addr.ip()) => Ok(Principal {
                subject: "localhost".into(),
                roles: vec!["local".into()],
            }),
            Some(addr) => Err(ServiceError::new(
                ServiceErrorKind::Unauthorized,
                format!("remote connection from {addr} rejected by default localhost-only policy"),
            )
            .with_hint("install a BearerTokenGate to enable remote access")),
        }
    }
}

// ── Bearer token gate (opt-in) ───────────────────────────────────────

/// Enterprise-ready bearer-token gate. Accepts `Authorization: Bearer
/// <token>` against a configured token list. Constant-time comparison
/// prevents timing-based token recovery.
#[derive(Debug, Clone)]
pub struct BearerTokenGate {
    tokens: Vec<String>,
}

impl BearerTokenGate {
    /// Create a gate that accepts any of `tokens`. An empty list is
    /// an error — it would allow zero requests through and is almost
    /// never what the caller intended.
    pub fn new(tokens: Vec<String>) -> Result<Self, &'static str> {
        if tokens.is_empty() {
            return Err("BearerTokenGate requires at least one token");
        }
        Ok(Self { tokens })
    }
}

fn constant_time_eq(a: &[u8], b: &[u8]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut diff = 0u8;
    for (x, y) in a.iter().zip(b.iter()) {
        diff |= x ^ y;
    }
    diff == 0
}

impl AuthGate for BearerTokenGate {
    fn authorize(&self, ctx: &AuthContext<'_>) -> Result<Principal, ServiceError> {
        let header = ctx.authorization.ok_or_else(|| {
            ServiceError::new(
                ServiceErrorKind::Unauthorized,
                "missing Authorization header",
            )
            .with_hint("send Authorization: Bearer <token>")
        })?;

        let token = header
            .strip_prefix("Bearer ")
            .or_else(|| header.strip_prefix("bearer "))
            .ok_or_else(|| {
                ServiceError::new(
                    ServiceErrorKind::Unauthorized,
                    "Authorization header must use the Bearer scheme",
                )
            })?;

        let token_bytes = token.as_bytes();
        for accepted in &self.tokens {
            if constant_time_eq(token_bytes, accepted.as_bytes()) {
                return Ok(Principal {
                    subject: "bearer".into(),
                    roles: vec!["api".into()],
                });
            }
        }

        Err(ServiceError::new(
            ServiceErrorKind::Unauthorized,
            "invalid bearer token",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::net::{Ipv4Addr, SocketAddr, SocketAddrV4};

    fn ctx(peer: Option<SocketAddr>, auth: Option<&str>) -> AuthContext<'_> {
        AuthContext {
            peer,
            authorization: auth,
            request_id: None,
        }
    }

    #[test]
    fn localhost_gate_allows_127_0_0_1() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::LOCALHOST, 1234));
        let g = AllowLocalhostGate;
        let p = g.authorize(&ctx(Some(addr), None)).unwrap();
        assert_eq!(p.subject, "localhost");
    }

    #[test]
    fn localhost_gate_rejects_remote() {
        let addr = SocketAddr::V4(SocketAddrV4::new(Ipv4Addr::new(10, 0, 0, 5), 1234));
        let err = AllowLocalhostGate
            .authorize(&ctx(Some(addr), None))
            .unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::Unauthorized);
        assert!(err.message.contains("10.0.0.5"));
    }

    #[test]
    fn localhost_gate_allows_missing_peer() {
        // Test harnesses that cannot extract peer must still pass.
        assert!(AllowLocalhostGate.authorize(&ctx(None, None)).is_ok());
    }

    #[test]
    fn bearer_gate_rejects_missing_header() {
        let g = BearerTokenGate::new(vec!["s3cret".into()]).unwrap();
        let err = g.authorize(&ctx(None, None)).unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::Unauthorized);
    }

    #[test]
    fn bearer_gate_accepts_valid_token() {
        let g = BearerTokenGate::new(vec!["s3cret".into()]).unwrap();
        let p = g.authorize(&ctx(None, Some("Bearer s3cret"))).unwrap();
        assert_eq!(p.roles, vec!["api"]);
    }

    #[test]
    fn bearer_gate_rejects_wrong_scheme() {
        let g = BearerTokenGate::new(vec!["s3cret".into()]).unwrap();
        let err = g.authorize(&ctx(None, Some("Basic s3cret"))).unwrap_err();
        assert_eq!(err.kind, ServiceErrorKind::Unauthorized);
    }

    #[test]
    fn bearer_gate_rejects_wrong_token() {
        let g = BearerTokenGate::new(vec!["s3cret".into()]).unwrap();
        assert!(g.authorize(&ctx(None, Some("Bearer nope"))).is_err());
    }

    #[test]
    fn bearer_gate_requires_tokens() {
        assert!(BearerTokenGate::new(vec![]).is_err());
    }

    #[test]
    fn constant_time_eq_matches_only_equal_bytes() {
        assert!(constant_time_eq(b"abc", b"abc"));
        assert!(!constant_time_eq(b"abc", b"abd"));
        assert!(!constant_time_eq(b"abc", b"abcd"));
    }
}
