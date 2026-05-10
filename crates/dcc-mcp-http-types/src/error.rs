//! Error types for the MCP HTTP server.
//!
//! Migrated to `dcc-mcp-http-types` (issue #852) so external Rust
//! consumers — REST clients, integration tests, gateway code that maps
//! HTTP errors into the cross-crate [`DccMcpError`] taxonomy (#488) —
//! can pull just the error surface without the full HTTP server crate.
//!
//! The full `dcc-mcp-http` crate re-exports [`HttpError`] / [`HttpResult`]
//! from `dcc_mcp_http::error` so historical call sites keep compiling
//! unchanged.

use dcc_mcp_models::DccMcpError;
use thiserror::Error;

/// Result type alias for HTTP server operations.
pub type HttpResult<T> = Result<T, HttpError>;

/// Errors produced by the MCP HTTP server surface.
///
/// Variants are intentionally fine-grained so the `axum` layer can map
/// each one to a precise HTTP status code (see [`Self::status_code`])
/// and so tests / orchestrators can branch on the cause without parsing
/// the human-readable message.
///
/// # Bubbling into the cross-crate taxonomy
///
/// Every variant has a documented mapping into [`DccMcpError`] via the
/// [`From`] impl below — see issue #488 for the rationale. Add a new
/// variant only when the existing taxonomy genuinely cannot carry the
/// caller's branching needs; otherwise extend [`HttpError::Internal`]
/// with a richer message.
#[must_use]
#[derive(Debug, Error)]
pub enum HttpError {
    /// The HTTP server is already running on this binding.
    #[error("server already running")]
    AlreadyRunning,

    /// The HTTP server has not been started yet.
    #[error("server not running")]
    NotRunning,

    /// The TCP listener could not bind to `addr`.
    #[error("failed to bind to {addr}: {source}")]
    BindFailed {
        /// The bind target that failed.
        addr: String,
        /// Underlying I/O error.
        #[source]
        source: std::io::Error,
    },

    /// The MCP `Mcp-Session-Id` did not resolve to a known session.
    #[error("session not found: {0}")]
    SessionNotFound(String),

    /// The MCP `Mcp-Session-Id` failed validation (wrong shape, etc).
    #[error("invalid session id: {0}")]
    InvalidSessionId(String),

    /// JSON serialisation / deserialisation error.
    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

    /// The DCC executor channel is closed (the worker exited).
    #[error("executor channel closed")]
    ExecutorClosed,

    /// The DCC main-thread queue refused a new task because it is at
    /// capacity and did not drain within the configured send timeout
    /// (issue #715).
    ///
    /// Distinct from [`Self::ExecutorClosed`] (dispatcher gone) so
    /// orchestrators can decide between "retry after `retry_after_secs`"
    /// vs. "fail over to a different backend". `depth` / `capacity` are
    /// exposed for operator diagnostics.
    #[error("queue overloaded (depth={depth}/{capacity}); retry in {retry_after_secs}s")]
    QueueOverloaded {
        /// Current queue depth at the moment the dispatch was rejected.
        depth: usize,
        /// Maximum queue depth.
        capacity: usize,
        /// Seconds the caller should wait before retrying.
        retry_after_secs: u64,
    },

    /// Action dispatch failed downstream of the HTTP server (e.g. the
    /// action raised, the bridge cancelled, etc).
    #[error("action dispatch error: {0}")]
    Dispatch(String),

    /// The dispatch did not complete within the configured timeout.
    #[error("request timeout after {ms}ms")]
    Timeout {
        /// Configured timeout in milliseconds.
        ms: u64,
    },

    /// The action exceeded its rate-limit quota.
    #[error("rate limit exceeded for action: {0}")]
    RateLimit(String),

    /// Catch-all for errors that do not fit any other variant. Add a
    /// new variant rather than reaching for this one when callers need
    /// to branch on the cause.
    #[error("internal error: {0}")]
    Internal(String),
}

impl HttpError {
    /// Convert to an HTTP status code.
    #[must_use]
    pub fn status_code(&self) -> u16 {
        match self {
            Self::SessionNotFound(_) => 404,
            Self::InvalidSessionId(_) => 400,
            Self::RateLimit(_) => 429,
            Self::AlreadyRunning | Self::NotRunning => 409,
            Self::Timeout { .. } => 504,
            Self::QueueOverloaded { .. } => 503,
            _ => 500,
        }
    }
}

/// Bubble `HttpError` into the shared `DccMcpError` taxonomy (#488).
///
/// Maps each fine-grained variant to the closest cross-crate kind so the
/// gateway / MCP error-code mapping stays consistent regardless of which
/// crate produced the error.
impl From<HttpError> for DccMcpError {
    fn from(err: HttpError) -> Self {
        match &err {
            HttpError::SessionNotFound(_) => DccMcpError::NotFound(err.to_string()),
            HttpError::InvalidSessionId(_) | HttpError::RateLimit(_) => {
                DccMcpError::Validation(err.to_string())
            }
            HttpError::Json(_) => DccMcpError::Serialization(err.to_string()),
            HttpError::BindFailed { .. } => DccMcpError::Io(err.to_string()),
            HttpError::Timeout { ms } => DccMcpError::Timeout { ms: *ms },
            _ => DccMcpError::Internal(err.to_string()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn session_not_found_maps_to_not_found() {
        let err: DccMcpError = HttpError::SessionNotFound("abc".into()).into();
        assert!(matches!(err, DccMcpError::NotFound(_)));
        assert_eq!(err.code(), "not_found");
    }

    #[test]
    fn invalid_session_id_maps_to_validation() {
        let err: DccMcpError = HttpError::InvalidSessionId("bad".into()).into();
        assert!(matches!(err, DccMcpError::Validation(_)));
    }

    #[test]
    fn timeout_carries_ms() {
        let err: DccMcpError = HttpError::Timeout { ms: 500 }.into();
        assert!(matches!(err, DccMcpError::Timeout { ms: 500 }));
    }

    #[test]
    fn bind_failed_maps_to_io() {
        let err: DccMcpError = HttpError::BindFailed {
            addr: "127.0.0.1:0".into(),
            source: std::io::Error::new(std::io::ErrorKind::AddrInUse, "addr in use"),
        }
        .into();
        assert!(matches!(err, DccMcpError::Io(_)));
    }

    #[test]
    fn already_running_maps_to_internal() {
        let err: DccMcpError = HttpError::AlreadyRunning.into();
        assert!(matches!(err, DccMcpError::Internal(_)));
    }

    // ── Status code mapping (regression guards for #488 contract) ──────

    #[test]
    fn status_codes_match_documented_contract() {
        assert_eq!(HttpError::SessionNotFound("x".into()).status_code(), 404);
        assert_eq!(HttpError::InvalidSessionId("x".into()).status_code(), 400);
        assert_eq!(HttpError::RateLimit("x".into()).status_code(), 429);
        assert_eq!(HttpError::AlreadyRunning.status_code(), 409);
        assert_eq!(HttpError::NotRunning.status_code(), 409);
        assert_eq!(HttpError::Timeout { ms: 1 }.status_code(), 504);
        assert_eq!(
            HttpError::QueueOverloaded {
                depth: 1,
                capacity: 1,
                retry_after_secs: 1,
            }
            .status_code(),
            503
        );
        assert_eq!(HttpError::ExecutorClosed.status_code(), 500);
        assert_eq!(HttpError::Internal("x".into()).status_code(), 500);
    }
}
