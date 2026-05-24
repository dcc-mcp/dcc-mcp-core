//! Domain layer for the DCC MCP gateway (issues #845, #852).
//!
//! # Clean Architecture — layer 0 (domain)
//!
//! This crate hosts the *pure types* that describe the gateway's domain
//! concepts. It intentionally has **no dependency** on:
//!
//! - `axum` / `reqwest` / `tokio` / any HTTP or async infrastructure,
//! - the file registry or any discovery transport,
//! - the broader `dcc-mcp-gateway` application crate.
//!
//! The dependency direction is strictly inward:
//!
//! ```text
//! dcc-mcp-gateway (app + infra)  →  dcc-mcp-gateway-core  (domain)
//! ```
//!
//! Consumers in `dcc-mcp-gateway` re-export these types under stable paths
//! so existing call sites keep compiling. New domain types should be added
//! here first, then re-exported.
//!
//! # Module map
//!
//! | Module           | What lives here                                     |
//! |------------------|-----------------------------------------------------|
//! | crate root       | [`PendingCall`] (routing primitive)                 |
//! | [`naming`]       | Pure UUID / alphabet helpers used by slug encoding  |
//! | [`resource_uri`] | Gateway resource URI prefix encode/decode helpers   |
//! | [`event`]        | Gateway contention event wire records               |
//! | [`openapi`]      | OpenAPI mount credential value types                |
//! | [`capability`]   | [`CapabilityRecord`] + slug encoding (REST wire)    |
//!
//! # Migration plan
//!
//! Types move here from `dcc-mcp-gateway` one at a time so each move can
//! be reviewed in isolation and the dependency direction verified. The
//! gateway crate re-exports every relocated type to preserve the public
//! API; downstream code that wants the smallest possible dependency
//! surface should import directly from `dcc_mcp_gateway_core`.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

pub mod capability;
pub mod event;
pub mod naming;
pub mod openapi;
pub mod policy;
pub mod resource_uri;

/// A call that the gateway has forwarded to a backend and is still awaiting
/// a response from.
///
/// Used by the routing layer so that an incoming `notifications/cancelled`
/// on the gateway's session can be translated into the correct backend
/// request id (see issue #321 for the cancellation correlation contract).
///
/// # Domain invariants
///
/// - `backend_url` is the fully-qualified URL the gateway forwarded the
///   call to (e.g. `http://127.0.0.1:18812/mcp`). It is *not* normalised —
///   callers store whatever URL they actually dispatched to so that
///   cancellations target the same physical endpoint.
/// - `backend_request_id` is the request id the backend sees, which may
///   differ from the gateway-side id when the gateway renames requests
///   for fan-out.
///
/// This is a pure value type: no `Arc`, no interior mutability, no
/// dependency on the routing layer's transport choice. Infrastructure code
/// owns the table that maps gateway ids to [`PendingCall`]s.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct PendingCall {
    /// URL of the backend that is servicing this call.
    pub backend_url: String,
    /// Request id as seen by the backend (may differ from the gateway id).
    pub backend_request_id: String,
}

impl PendingCall {
    /// Construct a new [`PendingCall`].
    ///
    /// Prefer this over struct-literal construction in new code so that any
    /// future validation (URL well-formedness, id non-emptiness) has a
    /// single place to live.
    #[must_use]
    pub fn new(backend_url: impl Into<String>, backend_request_id: impl Into<String>) -> Self {
        Self {
            backend_url: backend_url.into(),
            backend_request_id: backend_request_id.into(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_call_new_and_fields() {
        let c = PendingCall::new("http://127.0.0.1:18812/mcp", "req-42");
        assert_eq!(c.backend_url, "http://127.0.0.1:18812/mcp");
        assert_eq!(c.backend_request_id, "req-42");
    }

    #[test]
    fn pending_call_equality_is_structural() {
        let a = PendingCall::new("http://a", "1");
        let b = PendingCall::new("http://a", "1");
        let c = PendingCall::new("http://a", "2");
        assert_eq!(a, b);
        assert_ne!(a, c);
    }

    #[test]
    fn pending_call_roundtrip_json() {
        let c = PendingCall::new("http://127.0.0.1:18812/mcp", "req-7");
        let s = serde_json::to_string(&c).unwrap();
        let back: PendingCall = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
