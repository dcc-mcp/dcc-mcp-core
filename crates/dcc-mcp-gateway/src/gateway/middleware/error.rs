//! Error types for the middleware chain.

use thiserror::Error;

/// Errors that a middleware can return to abort the call pipeline.
#[must_use]
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

impl MiddlewareError {
    /// Small stable error kind for Admin, REST, Prometheus, and OTLP.
    #[must_use]
    pub fn kind(&self) -> &'static str {
        match self {
            Self::QuotaExceeded(_) => "throttled",
            Self::PolicyViolation(_) => "policy-denied",
            Self::Internal(_) => "middleware-error",
        }
    }

    /// Operator-facing category for governance telemetry.
    #[must_use]
    pub fn governance_category(&self) -> &'static str {
        match self {
            Self::QuotaExceeded(_) => "rate-limit",
            Self::PolicyViolation(_) => "policy",
            Self::Internal(_) => "middleware",
        }
    }
}
