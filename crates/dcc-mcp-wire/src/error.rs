//! Structured validation errors for wire-level checks.
//!
//! Consumers (clients, middleware, server handlers) get a stable
//! [`WireError`] variant instead of a free-form string, so they can
//! programmatically decide how to respond.

use thiserror::Error;

/// Structured error produced by wire-level validation.
///
/// Variants are intentionally fine-grained so callers can map them
/// to HTTP status codes or MCP error codes without string matching.
#[derive(Debug, Clone, Error)]
pub enum WireError {
    /// The `arguments` value was present but was not a JSON object.
    #[error("arguments must be a JSON object, got {kind}")]
    ArgumentsNotObject {
        /// JSON value kind that was received.
        kind: &'static str,
    },

    /// The `arguments` string could not be parsed as JSON.
    #[error("arguments string is not valid JSON: {reason}")]
    ArgumentsStringNotJson {
        /// Parser error or other diagnostic reason.
        reason: String,
    },

    /// The `arguments` string parsed successfully but decoded to a non-object.
    #[error("arguments decoded string is {kind}, expected object")]
    ArgumentsDecodedNotObject {
        /// JSON value kind produced by decoding the string.
        kind: &'static str,
    },

    /// A required field was missing from the request envelope.
    #[error("missing field `{field}` in request envelope")]
    MissingField {
        /// Missing field name or path.
        field: String,
    },

    /// The requested tool slug or name is invalid.
    #[error("tool_slug is invalid: {reason}")]
    InvalidToolSlug {
        /// Validation failure reason.
        reason: String,
    },

    /// Arguments failed validation against the tool input schema.
    #[error("input schema validation failed: {reason}")]
    SchemaValidationFailed {
        /// Schema validation failure reason.
        reason: String,
    },

    /// A payload was detected as double-stringified.
    #[error("double-stringified payload detected at `{path}`")]
    DoubleStringified {
        /// Field path where double-stringification was detected.
        path: String,
    },

    /// The request envelope itself was not a JSON object.
    #[error("request envelope is not a JSON object")]
    EnvelopeNotObject,

    /// A batch item failed wire-level validation.
    #[error("batch item {index}: {reason}")]
    BatchItemInvalid {
        /// Zero-based batch item index.
        index: usize,
        /// Item validation failure reason.
        reason: String,
    },
}

impl WireError {
    /// Stable machine-readable key for this error (snake_case).
    pub fn kind(&self) -> &'static str {
        match self {
            WireError::ArgumentsNotObject { .. } => "arguments-not-object",
            WireError::ArgumentsStringNotJson { .. } => "arguments-string-not-json",
            WireError::ArgumentsDecodedNotObject { .. } => "arguments-decoded-not-object",
            WireError::MissingField { .. } => "missing-field",
            WireError::InvalidToolSlug { .. } => "invalid-tool-slug",
            WireError::SchemaValidationFailed { .. } => "schema-validation-failed",
            WireError::DoubleStringified { .. } => "double-stringified",
            WireError::EnvelopeNotObject => "envelope-not-object",
            WireError::BatchItemInvalid { .. } => "batch-item-invalid",
        }
    }

    /// Short human-readable hint (suitable for `hint` field in REST errors).
    pub fn hint(&self) -> String {
        match self {
            WireError::ArgumentsNotObject { kind } => {
                format!("pass arguments as a JSON object {{}}, not {kind}")
            }
            WireError::ArgumentsStringNotJson { reason } => {
                format!("parse arguments string as JSON before sending: {reason}")
            }
            WireError::ArgumentsDecodedNotObject { kind } => {
                format!("the string in arguments field decoded to {kind}, expected object")
            }
            WireError::MissingField { field } => {
                format!("add `{field}` field to the request envelope")
            }
            WireError::InvalidToolSlug { reason } => {
                format!("use a valid tool slug from /v1/search: {reason}")
            }
            WireError::SchemaValidationFailed { reason } => {
                format!("check tool input schema via POST /v1/describe: {reason}")
            }
            WireError::DoubleStringified { path } => {
                format!("avoid double-serializing the payload at `{path}`")
            }
            WireError::EnvelopeNotObject => "the request body must be a JSON object".to_string(),
            WireError::BatchItemInvalid { index, reason } => {
                format!("fix item[{index}]: {reason}")
            }
        }
    }
}

/// Convenience alias: `Result<T, WireError>`.
pub type WireResult<T> = Result<T, WireError>;
