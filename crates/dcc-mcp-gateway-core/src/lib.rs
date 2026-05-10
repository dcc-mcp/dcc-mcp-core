//! Domain layer for the DCC MCP gateway (issue #845).
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
//! # What lives here (migration plan)
//!
//! This is the **first** landing zone — the follow-on PRs in the #845 chain
//! move capability-index / registry / model types here one at a time. The
//! current boundary line:
//!
//! | Lives here now            | Stays in `dcc-mcp-gateway` for now  |
//! |---------------------------|-------------------------------------|
//! | [`PendingCall`]           | `CapabilityIndex` (#845 Part 2)    |
//! |                           | `MiddlewareChain` (#770 follow-up)  |
//! |                           | `EventLog` (needs serde split)      |
//!
//! Picking [`PendingCall`] as the seed type is deliberate: it is the
//! smallest domain primitive with zero third-party dependencies, which lets
//! us verify the dependency direction (`dcc-mcp-gateway` depends on
//! `dcc-mcp-gateway-core`, never the other way) before larger types move.
//!
//! The `serde` feature is off by default; enable it when a downstream
//! application / infrastructure crate needs to serialize domain types.

#![forbid(unsafe_code)]
#![warn(missing_docs)]

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
#[derive(Debug, Clone, PartialEq, Eq)]
#[cfg_attr(feature = "serde", derive(serde::Serialize, serde::Deserialize))]
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

    #[cfg(feature = "serde")]
    #[test]
    fn pending_call_roundtrip_json() {
        let c = PendingCall::new("http://127.0.0.1:18812/mcp", "req-7");
        let s = serde_json::to_string(&c).unwrap();
        let back: PendingCall = serde_json::from_str(&s).unwrap();
        assert_eq!(c, back);
    }
}
