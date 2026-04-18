//! In-flight request tracking: progress notifications and cooperative cancellation.
//!
//! # Design
//!
//! Each `tools/call` request that is currently being dispatched has a
//! corresponding [`InFlightEntry`] stored in the [`InFlightRequests`] map.
//! The entry carries two shared objects:
//!
//! - [`CancelToken`]  — set by the notification handler when the client sends
//!   `notifications/cancelled`.  The executing action checks this flag and
//!   aborts early when set.
//! - [`ProgressReporter`] — used by the action handler (or wrappers around it)
//!   to emit `notifications/progress` events back to the client via SSE.
//!
//! ## Grace period (cancellation)
//!
//! Once [`CancelToken::cancel`] is called, the caller in `handle_tools_call`
//! waits up to [`CANCEL_GRACE_PERIOD`] for the dispatch future to complete
//! naturally (cooperative cancel).  If the future doesn't resolve within the
//! grace period, a `CANCELLED` error envelope is returned.

use dashmap::DashMap;
use serde_json::Value;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};

use crate::protocol::format_sse_event;
use crate::session::SessionManager;

/// Grace period before an in-flight tool call is hard-cancelled.
///
/// After [`CancelToken::cancel`] is called the `handle_tools_call` task waits
/// up to this duration for the dispatch future to return before giving up and
/// returning a `CANCELLED` error to the client.
pub const CANCEL_GRACE_PERIOD: std::time::Duration = std::time::Duration::from_secs(10);

// ── CancelToken ──────────────────────────────────────────────────────────────

/// A cooperative cancellation token shared between the HTTP handler and the
/// executing action.
///
/// # Usage
///
/// ```rust,ignore
/// // Handler: poll in the action loop
/// if cancel_token.is_cancelled() {
///     return Err("cancelled".to_string());
/// }
///
/// // Notification handler: signal cancellation
/// cancel_token.cancel();
/// ```
#[derive(Clone, Default)]
pub struct CancelToken(Arc<AtomicBool>);

impl CancelToken {
    /// Create a new token (initially not cancelled).
    pub fn new() -> Self {
        Self(Arc::new(AtomicBool::new(false)))
    }

    /// Signal that the associated request has been cancelled by the client.
    pub fn cancel(&self) {
        self.0.store(true, Ordering::Release);
    }

    /// Return `true` if cancellation has been requested.
    pub fn is_cancelled(&self) -> bool {
        self.0.load(Ordering::Acquire)
    }
}

// ── ProgressReporter ─────────────────────────────────────────────────────────

/// Sends `notifications/progress` events to the client via the session SSE
/// channel.
///
/// The reporter is a no-op when no `progressToken` was supplied by the client.
///
/// # MCP specification
///
/// ```json
/// {
///   "jsonrpc": "2.0",
///   "method": "notifications/progress",
///   "params": {
///     "progressToken": "<token from _meta>",
///     "progress": 42,
///     "total": 100,
///     "message": "optional human-readable status"
///   }
/// }
/// ```
#[derive(Clone)]
pub struct ProgressReporter {
    /// Echo-back of the client's `progressToken`. `None` means no-op.
    token: Option<Value>,
    /// MCP session ID for SSE push.
    session_id: Option<String>,
    /// Shared session manager — used to push events.
    sessions: SessionManager,
    /// Request ID used for logging.
    request_id: String,
}

impl ProgressReporter {
    /// Create a new reporter.
    ///
    /// If `token` is `None`, all `report()` calls are no-ops (backward compat).
    pub fn new(
        token: Option<Value>,
        session_id: Option<String>,
        sessions: SessionManager,
        request_id: String,
    ) -> Self {
        Self {
            token,
            session_id,
            sessions,
            request_id,
        }
    }

    /// Emit a progress event to the client.
    ///
    /// - `progress`: current work units completed (monotonically increasing).
    /// - `total`: total work units, or `None` if unknown.
    /// - `message`: optional human-readable status string.
    ///
    /// No-op when no `progressToken` was provided or session has no SSE stream.
    pub fn report(&self, progress: f64, total: Option<f64>, message: Option<&str>) {
        let (Some(token), Some(sid)) = (self.token.as_ref(), self.session_id.as_ref()) else {
            return;
        };

        let mut params = serde_json::json!({
            "progressToken": token,
            "progress": progress,
        });

        if let Some(t) = total {
            params["total"] = serde_json::json!(t);
        }
        if let Some(m) = message {
            params["message"] = serde_json::json!(m);
        }

        let notification = serde_json::json!({
            "jsonrpc": "2.0",
            "method": "notifications/progress",
            "params": params,
        });

        let event = format_sse_event(&notification, None);
        self.sessions.push_event(sid, event);

        tracing::debug!(
            request_id = %self.request_id,
            progress = progress,
            total = ?total,
            "progress notification sent"
        );
    }

    /// Convenience: report with percentage (0.0–100.0).
    pub fn report_pct(&self, pct: f64, message: Option<&str>) {
        self.report(pct, Some(100.0), message);
    }
}

// ── InFlightEntry ─────────────────────────────────────────────────────────────

/// State record for a single in-flight `tools/call`.
pub struct InFlightEntry {
    /// Cancellation token — set externally by the notification handler.
    pub cancel_token: CancelToken,
    /// Progress reporter — caller uses this to emit progress events.
    pub progress: ProgressReporter,
    /// Wall-clock time when the request started (for TTL / metrics).
    pub started_at: std::time::Instant,
}

impl InFlightEntry {
    pub fn new(cancel_token: CancelToken, progress: ProgressReporter) -> Self {
        Self {
            cancel_token,
            progress,
            started_at: std::time::Instant::now(),
        }
    }
}

// ── InFlightRequests ─────────────────────────────────────────────────────────

/// Thread-safe map from request-id → [`InFlightEntry`].
///
/// Entries are inserted before dispatch and removed on completion.
#[derive(Clone, Default)]
pub struct InFlightRequests {
    map: Arc<DashMap<String, InFlightEntry>>,
}

impl InFlightRequests {
    pub fn new() -> Self {
        Self {
            map: Arc::new(DashMap::new()),
        }
    }

    /// Register a new in-flight request.
    pub fn insert(&self, request_id: String, entry: InFlightEntry) {
        self.map.insert(request_id, entry);
    }

    /// Remove a completed request and return its entry.
    pub fn remove(&self, request_id: &str) -> Option<(String, InFlightEntry)> {
        self.map.remove(request_id)
    }

    /// Set the cancel flag for an in-flight request.
    ///
    /// Returns `true` if the request was found and the flag was set.
    pub fn request_cancel(&self, request_id: &str) -> bool {
        if let Some(entry) = self.map.get(request_id) {
            entry.cancel_token.cancel();
            true
        } else {
            false
        }
    }

    /// Purge entries that have been running longer than `max_age`.
    pub fn purge_stale(&self, max_age: std::time::Duration) {
        self.map.retain(|_, e| e.started_at.elapsed() < max_age);
    }

    /// Current count of in-flight requests.
    pub fn len(&self) -> usize {
        self.map.len()
    }

    /// `true` when there are no in-flight requests.
    pub fn is_empty(&self) -> bool {
        self.map.is_empty()
    }
}
