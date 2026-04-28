//! Shared `DccMcpError` for cross-crate error bubbling (#488).
//!
//! Most crates in the workspace define their own `Error` enum with overlapping
//! variants (`Io`, `Json`, `Internal`, `NotFound`, `Timeout`). Bubbling an
//! error through three crates used to require three explicit `Into`
//! conversions and three serialisation paths at the gateway boundary.
//!
//! `DccMcpError` is the *lingua franca* for those crossings. Crate-local
//! error enums keep their fine-grained variants but gain a single
//! `impl From<MyCrateError> for DccMcpError`, so a `?` at a crate boundary
//! is enough to convert. The gateway can then format any error from any
//! crate through one consistent code-mapping.

use std::fmt;

use serde::{Deserialize, Serialize};
use thiserror::Error;

/// Cross-crate error type used at the gateway boundary.
///
/// Variants are deliberately coarse — they exist to classify an error for
/// the gateway / MCP error code mapping, not to replace per-crate
/// fine-grained errors. Crate-local errors should keep their own typed
/// enums and convert *into* `DccMcpError` only when bubbling across a
/// crate boundary.
#[derive(Debug, Clone, PartialEq, Eq, Error, Serialize, Deserialize)]
#[serde(tag = "kind", content = "message")]
pub enum DccMcpError {
    /// IO failure (file not found, permission denied, broken pipe, …).
    #[error("io: {0}")]
    Io(String),

    /// JSON / serialisation failure (malformed input, missing field, …).
    #[error("serialization: {0}")]
    Serialization(String),

    /// Caller-side validation failure (bad parameters, schema mismatch, …).
    #[error("validation: {0}")]
    Validation(String),

    /// Requested entity does not exist.
    #[error("not found: {0}")]
    NotFound(String),

    /// Operation exceeded its time budget.
    #[error("timeout after {ms}ms")]
    Timeout {
        /// How long we waited before giving up.
        ms: u64,
    },

    /// Internal failure that does not fit any other variant.
    #[error("internal: {0}")]
    Internal(String),
}

impl DccMcpError {
    /// Stable string code that the gateway maps to an MCP error code.
    ///
    /// Codes are stable across releases — adding a new variant means
    /// adding a new code, never renaming an existing one.
    pub fn code(&self) -> &'static str {
        match self {
            Self::Io(_) => "io",
            Self::Serialization(_) => "serialization",
            Self::Validation(_) => "validation",
            Self::NotFound(_) => "not_found",
            Self::Timeout { .. } => "timeout",
            Self::Internal(_) => "internal",
        }
    }

    /// Build an [`Internal`](Self::Internal) error from any displayable value.
    pub fn internal<E: fmt::Display>(err: E) -> Self {
        Self::Internal(err.to_string())
    }

    /// Build an [`Io`](Self::Io) error from any displayable value.
    pub fn io<E: fmt::Display>(err: E) -> Self {
        Self::Io(err.to_string())
    }

    /// Build a [`Validation`](Self::Validation) error from any displayable value.
    pub fn validation<E: fmt::Display>(err: E) -> Self {
        Self::Validation(err.to_string())
    }

    /// Build a [`NotFound`](Self::NotFound) error for the named entity.
    pub fn not_found<S: Into<String>>(what: S) -> Self {
        Self::NotFound(what.into())
    }
}

impl From<std::io::Error> for DccMcpError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}

impl From<serde_json::Error> for DccMcpError {
    fn from(e: serde_json::Error) -> Self {
        Self::Serialization(e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn code_is_stable() {
        assert_eq!(DccMcpError::Io("x".into()).code(), "io");
        assert_eq!(
            DccMcpError::Serialization("x".into()).code(),
            "serialization"
        );
        assert_eq!(DccMcpError::Validation("x".into()).code(), "validation");
        assert_eq!(DccMcpError::NotFound("x".into()).code(), "not_found");
        assert_eq!(DccMcpError::Timeout { ms: 100 }.code(), "timeout");
        assert_eq!(DccMcpError::Internal("x".into()).code(), "internal");
    }

    #[test]
    fn from_io_error_classifies_as_io() {
        let io: std::io::Error = std::io::Error::new(std::io::ErrorKind::NotFound, "missing");
        let err: DccMcpError = io.into();
        assert!(matches!(err, DccMcpError::Io(_)));
    }

    #[test]
    fn display_includes_message() {
        let err = DccMcpError::Validation("bad input".into());
        assert_eq!(err.to_string(), "validation: bad input");
    }

    #[test]
    fn json_round_trip_preserves_variant() {
        let err = DccMcpError::Timeout { ms: 250 };
        let json = serde_json::to_string(&err).unwrap();
        let back: DccMcpError = serde_json::from_str(&json).unwrap();
        assert_eq!(err, back);
    }
}
