//! Error types for the workflow crate.

use thiserror::Error;

/// Validation failure when checking a [`crate::WorkflowSpec`].
#[derive(Debug, Error, Clone, PartialEq, Eq)]
pub enum ValidationError {
    /// The spec declares no steps.
    #[error("workflow spec must declare at least one step")]
    NoSteps,

    /// Two or more steps share the same id.
    #[error("duplicate step id: {0:?}")]
    DuplicateStepId(String),

    /// A step id is empty.
    #[error("empty step id")]
    EmptyStepId,

    /// A `tool` step references a name that is not a valid MCP tool name
    /// per `dcc-mcp-naming::validate_tool_name`.
    #[error("step {step_id:?}: invalid tool name {tool:?}: {reason}")]
    InvalidToolName {
        /// Step that triggered the failure.
        step_id: String,
        /// Offending tool name.
        tool: String,
        /// Human-readable reason from the naming crate.
        reason: String,
    },

    /// A `branch.on` or `foreach.items` expression does not parse as JSONPath.
    #[error("step {step_id:?}: invalid JSONPath expression {expr:?}: {reason}")]
    InvalidJsonPath {
        /// Step that triggered the failure.
        step_id: String,
        /// Offending expression.
        expr: String,
        /// Parser error string.
        reason: String,
    },

    /// A kind-specific field is missing (e.g. `foreach` without `items`).
    #[error("step {step_id:?} ({kind}): missing required field {field:?}")]
    MissingField {
        /// Step id.
        step_id: String,
        /// Step kind rendered as lowercase string.
        kind: &'static str,
        /// Missing field name.
        field: &'static str,
    },
}

/// Top-level error type returned by workflow operations.
#[derive(Debug, Error)]
pub enum WorkflowError {
    /// YAML deserialisation failure.
    #[error("yaml parse error: {0}")]
    Yaml(String),

    /// Validation failed after parsing.
    #[error("validation failed: {0}")]
    Validation(#[from] ValidationError),

    /// An operation is declared but not yet implemented in this skeleton.
    ///
    /// This is the stable error returned by the three execution-facing
    /// built-in tools (`workflows.run` / `workflows.get_status` /
    /// `workflows.cancel`) so downstream callers can depend on a fixed
    /// shape. See issue #348.
    #[error("not implemented: {0}")]
    NotImplemented(&'static str),

    /// I/O error while reading a workflow file.
    #[error("io error: {0}")]
    Io(String),
}

impl From<std::io::Error> for WorkflowError {
    fn from(e: std::io::Error) -> Self {
        Self::Io(e.to_string())
    }
}
