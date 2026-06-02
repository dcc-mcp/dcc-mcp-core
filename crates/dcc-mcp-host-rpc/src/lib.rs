//! `dcc-mcp-host-rpc` — out-of-process **sidecar → DCC** RPC contract.
//!
//! This crate is the **Phase 2 substrate** for the sidecar epic
//! ([RFC #998](https://github.com/loonghao/dcc-mcp-core/issues/998),
//! tracked under [#1002](https://github.com/loonghao/dcc-mcp-core/issues/1002)).
//! It defines the abstract trait every per-DCC sidecar implementation
//! satisfies, the concrete clients the generic sidecar can route to today,
//! and the structured error envelope the gateway propagates as the
//! `host_died` event.
//!
//! # What lives where
//!
//! * **This crate**: trait + envelope types + URI scheme registry + bundled
//!   `commandport://`, `qtserver://`, `ws://`, `wss://`, and `stub://`
//!   clients.
//! * `dcc-mcp-server` `sidecar` subcommand (issue #1002 / PR #1003): owns the
//!   process lifecycle, PPID-watch, and `FileRegistry` registration. Calls
//!   into a [`HostRpcClient`] impl chosen by the `--host-rpc` URI scheme.
//! * Per-DCC adapter repos (`dcc-mcp-maya`, `dcc-mcp-blender`, ...): ship
//!   the host-side dispatcher/bridge that those clients call inside the live
//!   DCC process (Maya bootstrap module, Qt server entry point, WebSocket
//!   handler, Houdini `hrpyc` bridge, Unreal Remote Execution Server, etc.).
//!
//! # Why a trait, not a concrete type
//!
//! The sidecar binary is **DCC-agnostic** by design. There is exactly one
//! Rust binary shipped via PyPI (`dcc-mcp-server`); it serves Maya,
//! Blender, Houdini, Unreal, 3ds Max, Photoshop, Figma, ZBrush, etc.
//! The only delta per DCC is the **wire format** of the channel back to
//! the live host process — a trait is the cleanest way to make those
//! impls pluggable while keeping the sidecar lifecycle code in one place.
//!
//! # Current status
//!
//! The generic sidecar can instantiate clients by URI scheme through
//! [`client_for_uri`]:
//!
//! * `commandport://` — Maya commandPort client. At connect time it injects
//!   the bundled Maya bootstrap, then dispatches through
//!   `dcc_mcp_maya._sidecar.dispatch(...)`.
//! * `qtserver://` — line-oriented JSON client for DCC plugins that expose a
//!   local Qt/TCP dispatcher.
//! * `ws://` / `wss://` — JSON WebSocket client for adapters with an embedded
//!   WebSocket dispatcher.
//! * `stub://` — deterministic test client.
//!
//! The included [`StubHostRpcClient`] is a deterministic placeholder: it
//! accepts connect attempts, but calls always return
//! `HostRpcError::TransportError("stub client")` so integration tests for the
//! sidecar binary can exercise the call path without depending on a real DCC.
//! [`UnavailableHostRpcClient`] is the diagnostic placeholder used after the
//! sidecar already knows host RPC cannot be reached; calls return the original
//! startup failure as a stable transport error.
//!
//! # Example
//!
//! ```rust,no_run
//! use dcc_mcp_host_rpc::{HostRpcClient, HostRpcError, StubHostRpcClient};
//! use std::time::Duration;
//!
//! # async fn demo() -> Result<(), HostRpcError> {
//! let mut client = StubHostRpcClient::new();
//! client
//!     .connect("commandport://127.0.0.1:6000", Duration::from_secs(5))
//!     .await?;
//!
//! let request_id = "req-1";
//! let result = client
//!     .call(
//!         "maya_render__playblast",
//!         serde_json::json!({"width": 1280, "height": 720}),
//!         request_id,
//!     )
//!     .await;
//!
//! match result {
//!     Ok(payload) => println!("ok: {payload}"),
//!     Err(HostRpcError::HostDied {
//!         last_call_slug,
//!         last_call_args,
//!     }) => {
//!         eprintln!(
//!             "host died during {last_call_slug:?} with args {last_call_args:?}"
//!         );
//!     }
//!     Err(other) => return Err(other),
//! }
//! # Ok(())
//! # }
//! ```

#![deny(missing_docs)]

pub mod commandport;
pub mod qtserver;
pub mod registry;
pub mod websocket;

pub use commandport::CommandPortClient;
pub use qtserver::QtServerClient;
pub use registry::{client_for_uri, parse_scheme, registered_schemes};
pub use websocket::WebSocketHostRpcClient;

use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

// ── HostRpcError ──────────────────────────────────────────────────────────

/// Structured failure modes a [`HostRpcClient`] reports back to the
/// sidecar dispatcher.
///
/// The variants are deliberately coarse-grained so the gateway can
/// translate each into a stable wire envelope (`host-died`,
/// `transport-error`, `timeout`, …) without leaking transport-specific
/// noise to MCP clients.
///
/// # Wire format
///
/// All variants are internally tagged on `kind`, with kebab-case tags
/// (`"host-died"` / `"transport-error"` / `"timeout"` / `"cancelled"`
/// / `"backend-error"`).  Every variant uses **struct-style** fields
/// (no newtype tuples) so `serde(tag = …)` round-trips cleanly — pin
/// this at the type level rather than per-variant so future additions
/// stay consistent.
#[derive(Debug, thiserror::Error, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum HostRpcError {
    /// The DCC process died (or its RPC channel closed permanently)
    /// while we held a request.  This is the **canonical sidecar
    /// signal**: it lets the gateway emit a `host_died` SSE event so
    /// agents stop seeing transport-error cascades.
    ///
    /// The `last_call_*` fields capture the in-flight request so the
    /// gateway audit log can record it for postmortem analysis (Phase 3
    /// of #998).
    #[error("host process died during {last_call_slug:?}")]
    HostDied {
        /// Slug of the in-flight tool call when the host died, when known.
        #[serde(rename = "last-call-slug", skip_serializing_if = "Option::is_none")]
        last_call_slug: Option<String>,
        /// Arguments for the in-flight call, when known.  Truncated /
        /// redacted by the audit middleware before reaching the public
        /// event stream.
        #[serde(rename = "last-call-args", skip_serializing_if = "Option::is_none")]
        last_call_args: Option<serde_json::Value>,
    },

    /// Lower-level transport failure — connection refused, TLS error,
    /// malformed frame, etc.  Distinct from [`HostRpcError::HostDied`]
    /// because the host process may still be alive (just unreachable).
    #[error("transport error: {message}")]
    TransportError {
        /// Human-readable description of the underlying transport
        /// failure.  Not parsed by the gateway — surfaced verbatim in
        /// the structured error envelope sent to MCP clients.
        message: String,
    },

    /// The request did not return a result before its budget elapsed.
    /// Sidecars surface this when their per-call watchdog fires.
    #[error("timeout")]
    Timeout {},

    /// The caller (or a cancellation propagated from MCP) signalled
    /// cancellation; the sidecar honoured it and returned without a
    /// result.
    #[error("cancelled")]
    Cancelled {},

    /// The DCC executed the call and returned a structured failure
    /// envelope of its own (e.g. a skill returning
    /// `{"success": false, "error": "..."}`).  The payload is the raw
    /// envelope so the gateway can forward it untouched.
    #[error("backend error")]
    BackendError {
        /// Backend's structured failure envelope, forwarded as-is.
        envelope: serde_json::Value,
    },
}

impl HostRpcError {
    /// Convenience constructor for the most common case.
    #[must_use]
    pub fn host_died(slug: impl Into<String>, args: Option<serde_json::Value>) -> Self {
        Self::HostDied {
            last_call_slug: Some(slug.into()),
            last_call_args: args,
        }
    }

    /// Convenience constructor for the unknown-in-flight-call case
    /// (e.g. PPID-watch detected the parent died while no call was
    /// active).
    #[must_use]
    pub fn host_died_idle() -> Self {
        Self::HostDied {
            last_call_slug: None,
            last_call_args: None,
        }
    }

    /// Convenience constructor for a transport-level failure.
    #[must_use]
    pub fn transport(message: impl Into<String>) -> Self {
        Self::TransportError {
            message: message.into(),
        }
    }

    /// Convenience constructor for a backend-side structured failure.
    #[must_use]
    pub fn backend(envelope: serde_json::Value) -> Self {
        Self::BackendError { envelope }
    }

    /// Whether this error represents a **terminal** host failure that
    /// should make the gateway evict the backend from the routing
    /// table.
    #[must_use]
    pub fn is_terminal(&self) -> bool {
        matches!(self, Self::HostDied { .. })
    }
}

// ── HostRpcClient trait ───────────────────────────────────────────────────

/// Sidecar-side handle that talks to the live DCC over its native
/// command channel.
///
/// Per-DCC implementations live in their adapter repos.  This trait is
/// the **only** thing the sidecar binary needs to know about — the
/// concrete impl is selected at startup from the `--host-rpc <URI>`
/// argument's scheme.
///
/// # Threading
///
/// Every method is `async` and takes `&self` (or `&mut self` for
/// `connect`). Implementations are expected to be `Send + Sync` so the
/// sidecar's Tokio scheduler can drive multiple in-flight calls (when
/// the underlying transport is duplexed; serial-only transports like
/// raw `commandPort` should serialise internally).
///
/// # Cancellation
///
/// [`Self::cancel`] is a best-effort hint — implementations should
/// abort the in-flight call when the underlying RPC supports it (Maya
/// `commandPort` does not; Houdini `hrpyc` does), and otherwise return
/// `Ok(())` and let the call complete naturally.
#[async_trait]
pub trait HostRpcClient: Send + Sync {
    /// URI scheme this implementation handles
    /// (e.g. `"commandport"`, `"hrpyc"`, `"http"`).
    ///
    /// Used by the sidecar's URI router to dispatch `--host-rpc` to
    /// the correct impl. Must be lowercase, ASCII, no `://`.
    fn uri_scheme(&self) -> &'static str;

    /// Open the channel to the live DCC process.
    ///
    /// `endpoint` is the full URI passed via `--host-rpc` (the impl
    /// receives the whole thing — scheme, host, port, query string).
    /// `timeout` bounds connect attempts; the sidecar passes a
    /// per-environment value chosen via env / CLI.
    async fn connect(&mut self, endpoint: &str, timeout: Duration) -> Result<(), HostRpcError>;

    /// Dispatch a single tool call through to the DCC.
    ///
    /// `action` is the per-DCC backend tool name (`maya_render__playblast`,
    /// `bpy_ops__import_fbx`, …) — *not* the gateway-prefixed slug.
    /// `request_id` is the MCP request id so [`Self::cancel`] and audit
    /// correlation work.
    async fn call(
        &self,
        action: &str,
        args: serde_json::Value,
        request_id: &str,
    ) -> Result<serde_json::Value, HostRpcError>;

    /// Dispatch a call with optional trace context. Implementations that do
    /// not have a wire slot for this metadata can rely on the default fallback.
    async fn call_with_trace_context(
        &self,
        action: &str,
        args: serde_json::Value,
        request_id: &str,
        _trace_context: Option<serde_json::Value>,
    ) -> Result<serde_json::Value, HostRpcError> {
        self.call(action, args, request_id).await
    }

    /// Best-effort cancellation hint for the given in-flight call.
    /// Defaults to `Ok(())` so impls that cannot cancel don't have to
    /// override.
    async fn cancel(&self, _request_id: &str) -> Result<(), HostRpcError> {
        Ok(())
    }

    /// Transport-level liveness probe.  Returns `false` once the impl
    /// has observed a permanent disconnect; the sidecar's PPID-watch
    /// is the orthogonal signal.
    fn is_alive(&self) -> bool;

    /// Tear down the channel.  Implementations should be idempotent so
    /// the sidecar can call this from both the parent-death and
    /// signal-handling paths without coordination.
    async fn close(&self);
}

// ── UnavailableHostRpcClient ──────────────────────────────────────────────

/// Permanently-unavailable diagnostic client for sidecar startup failures.
///
/// The generic sidecar uses this when the configured host RPC URI is
/// unsupported or the initial connect attempt fails. It lets the sidecar still
/// bind its MCP dispatch listener and answer `tools/call` with a structured
/// [`HostRpcError::TransportError`] instead of leaving clients with only a
/// missing socket.
#[derive(Debug, Clone)]
pub struct UnavailableHostRpcClient {
    reason: String,
}

impl UnavailableHostRpcClient {
    /// Construct a diagnostic client that will report `reason` on every call.
    #[must_use]
    pub fn new(reason: impl Into<String>) -> Self {
        Self {
            reason: reason.into(),
        }
    }

    /// Human-readable startup failure that made this client unavailable.
    #[must_use]
    pub fn reason(&self) -> &str {
        &self.reason
    }
}

#[async_trait]
impl HostRpcClient for UnavailableHostRpcClient {
    fn uri_scheme(&self) -> &'static str {
        "unavailable"
    }

    async fn connect(&mut self, _endpoint: &str, _timeout: Duration) -> Result<(), HostRpcError> {
        Err(HostRpcError::transport(self.reason.clone()))
    }

    async fn call(
        &self,
        _action: &str,
        _args: serde_json::Value,
        _request_id: &str,
    ) -> Result<serde_json::Value, HostRpcError> {
        Err(HostRpcError::transport(self.reason.clone()))
    }

    fn is_alive(&self) -> bool {
        false
    }

    async fn close(&self) {}
}

// ── StubHostRpcClient ─────────────────────────────────────────────────────

/// Reference implementation that **never connects**, used by sidecar
/// integration tests and as a starting point for new per-DCC impls.
///
/// All `call`s return [`HostRpcError::TransportError`] with a
/// `"stub client"` payload so integration tests can confirm the
/// sidecar lifecycle (PPID-watch, registry registration, error
/// propagation) end-to-end without depending on a real DCC.
///
/// Use this as the seed when writing a real per-DCC impl: copy the
/// file, change `uri_scheme`, and replace the `Mutex<bool>` state with
/// an actual transport handle.
#[derive(Debug, Default)]
pub struct StubHostRpcClient {
    connected: AtomicBool,
    calls: Mutex<Vec<String>>,
}

impl StubHostRpcClient {
    /// Construct a fresh stub client — never connected.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot of every `action` value passed through `call()` since
    /// construction.  Useful in tests that need to assert "the sidecar
    /// dispatched the right slug".
    #[must_use]
    pub fn observed_calls(&self) -> Vec<String> {
        self.calls.lock().expect("stub mutex").clone()
    }
}

#[async_trait]
impl HostRpcClient for StubHostRpcClient {
    fn uri_scheme(&self) -> &'static str {
        "stub"
    }

    async fn connect(&mut self, _endpoint: &str, _timeout: Duration) -> Result<(), HostRpcError> {
        self.connected.store(true, Ordering::SeqCst);
        Ok(())
    }

    async fn call(
        &self,
        action: &str,
        _args: serde_json::Value,
        _request_id: &str,
    ) -> Result<serde_json::Value, HostRpcError> {
        self.calls
            .lock()
            .expect("stub mutex")
            .push(action.to_string());
        Err(HostRpcError::transport("stub client"))
    }

    fn is_alive(&self) -> bool {
        self.connected.load(Ordering::SeqCst)
    }

    async fn close(&self) {
        self.connected.store(false, Ordering::SeqCst);
    }
}

// ── tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::time::Duration;

    #[tokio::test]
    async fn stub_client_lifecycle() {
        let mut client = StubHostRpcClient::new();
        assert!(!client.is_alive());
        assert_eq!(client.observed_calls(), Vec::<String>::new());

        client
            .connect("stub://localhost", Duration::from_millis(10))
            .await
            .expect("stub connect must succeed");
        assert!(client.is_alive());

        let result = client
            .call("maya_render__playblast", serde_json::json!({}), "req-1")
            .await;
        assert!(matches!(result, Err(HostRpcError::TransportError { .. })));
        assert_eq!(client.observed_calls(), vec!["maya_render__playblast"]);

        // cancel default is a no-op
        client.cancel("req-1").await.expect("cancel default Ok");

        client.close().await;
        assert!(!client.is_alive());
    }

    #[test]
    fn host_died_constructors() {
        let err = HostRpcError::host_died("playblast", Some(serde_json::json!({"width": 1280})));
        match err {
            HostRpcError::HostDied {
                last_call_slug,
                last_call_args,
            } => {
                assert_eq!(last_call_slug.as_deref(), Some("playblast"));
                assert!(last_call_args.is_some());
            }
            other => panic!("expected HostDied, got {other:?}"),
        }
        assert!(HostRpcError::host_died_idle().is_terminal());
        assert!(!HostRpcError::Timeout {}.is_terminal());
        assert!(!HostRpcError::Cancelled {}.is_terminal());
        assert!(!HostRpcError::transport("x").is_terminal());
    }

    #[test]
    fn host_rpc_error_serde_roundtrip_host_died() {
        // The HostDied envelope is the wire-format the gateway emits as
        // the public `host_died` SSE event — pin its on-the-wire shape
        // so downstream consumers (admin UI, MCP clients) cannot break
        // silently.
        let err = HostRpcError::host_died(
            "maya_render__playblast",
            Some(serde_json::json!({"width": 1280, "height": 720})),
        );
        let json = serde_json::to_value(&err).expect("serialise");
        assert_eq!(json["kind"], "host-died");
        assert_eq!(json["last-call-slug"], "maya_render__playblast");
        assert_eq!(json["last-call-args"]["width"], 1280);

        let roundtrip: HostRpcError = serde_json::from_value(json).expect("deserialise");
        match roundtrip {
            HostRpcError::HostDied {
                last_call_slug,
                last_call_args,
            } => {
                assert_eq!(last_call_slug.as_deref(), Some("maya_render__playblast"));
                assert_eq!(last_call_args.unwrap()["height"].as_i64(), Some(720));
            }
            other => panic!("expected HostDied, got {other:?}"),
        }
    }

    #[test]
    fn host_rpc_error_serde_roundtrip_other_variants() {
        for (err, expected_kind) in [
            (
                HostRpcError::transport("connection refused"),
                "transport-error",
            ),
            (HostRpcError::Timeout {}, "timeout"),
            (HostRpcError::Cancelled {}, "cancelled"),
            (
                HostRpcError::backend(serde_json::json!({"success": false, "error": "boom"})),
                "backend-error",
            ),
        ] {
            let json = serde_json::to_value(&err).expect("serialise");
            assert_eq!(json["kind"], expected_kind, "{err:?}");
            let _: HostRpcError = serde_json::from_value(json).expect("deserialise");
        }
    }

    #[test]
    fn is_terminal_only_true_for_host_died() {
        assert!(HostRpcError::host_died("x", None).is_terminal());
        assert!(!HostRpcError::transport("x").is_terminal());
        assert!(!HostRpcError::Timeout {}.is_terminal());
        assert!(!HostRpcError::Cancelled {}.is_terminal());
        assert!(!HostRpcError::backend(serde_json::json!({})).is_terminal());
    }

    #[tokio::test]
    async fn stub_uri_scheme_is_stable() {
        // Pin the scheme tag so the URI router in dcc-mcp-server can
        // dispatch on it without breaking when this crate ships.
        let client = StubHostRpcClient::new();
        assert_eq!(client.uri_scheme(), "stub");
    }

    #[tokio::test]
    async fn unavailable_client_reports_startup_failure() {
        let reason = "host-rpc connect to `commandport://127.0.0.1:1` failed";
        let mut client = UnavailableHostRpcClient::new(reason);
        assert_eq!(client.uri_scheme(), "unavailable");
        assert_eq!(client.reason(), reason);
        assert!(!client.is_alive());

        let connect = client
            .connect("unavailable://diagnostic", Duration::from_millis(10))
            .await;
        assert!(
            matches!(
                connect,
                Err(HostRpcError::TransportError { ref message }) if message.contains(reason)
            ),
            "connect should preserve the startup failure: {connect:?}"
        );

        let call = client
            .call(
                "maya_primitives__create_sphere",
                serde_json::json!({}),
                "req",
            )
            .await;
        assert!(
            matches!(
                call,
                Err(HostRpcError::TransportError { ref message }) if message.contains(reason)
            ),
            "call should preserve the startup failure: {call:?}"
        );
    }
}
