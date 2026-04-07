//! Error types for the MCP HTTP server.

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
            _ => 500,
        }
    }
}
