//! Error types for the middleware chain.

use thiserror::Error;

/// Errors that a middleware can return to abort the call pipeline.
#[derive(Debug, Error, Clone)]
pub enum MiddlewareError {
    /// Quota limit exceeded (e.g. too many calls per minute).
    #[error("quota exceeded: {0}")]
    QuotaExceeded(String),

    /// A required field or policy was violated.
    #[error("policy violation: {0}")]
    PolicyViolation(String),

    /// The middleware encountered an internal error.
    #[error("middleware error: {0}")]
    Internal(String),
}
