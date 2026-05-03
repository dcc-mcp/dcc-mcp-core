//! Error types for the MCP HTTP server.

use dcc_mcp_models::DccMcpError;
use thiserror::Error;

pub type HttpResult<T> = Result<T, HttpError>;

#[derive(Debug, Error)]
pub enum HttpError {
    #[error("server already running")]
    AlreadyRunning,

    #[error("server not running")]
    NotRunning,

    #[error("failed to bind to {addr}: {source}")]
    BindFailed {
        addr: String,
        #[source]
        source: std::io::Error,
    },

    #[error("session not found: {0}")]
    SessionNotFound(String),

    #[error("invalid session id: {0}")]
    InvalidSessionId(String),

    #[error("JSON serialization error: {0}")]
    Json(#[from] serde_json::Error),

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
        depth: usize,
        capacity: usize,
        retry_after_secs: u64,
    },

    #[error("action dispatch error: {0}")]
    Dispatch(String),

    #[error("request timeout after {ms}ms")]
    Timeout { ms: u64 },

    #[error("rate limit exceeded for action: {0}")]
    RateLimit(String),

    #[error("internal error: {0}")]
    Internal(String),
}

impl HttpError {
    /// Convert to an HTTP status code.
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
}
