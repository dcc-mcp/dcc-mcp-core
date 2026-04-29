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

    /// A per-step policy (timeout, retry, idempotency) is malformed
    /// beyond what serde itself can catch.
    ///
    /// Examples: `retry.max_attempts == 0`, `timeout_secs == 0`,
    /// `initial_delay > max_delay`.
    #[error("step {step_id:?}: invalid policy: {reason}")]
    InvalidPolicy {
        /// Step that triggered the failure.
        step_id: String,
        /// Human-readable reason.
        reason: String,
    },

    /// An `idempotency_key` template references an identifier that is
    /// neither a workflow input nor a prior step id.
    #[error("step {step_id:?}: idempotency_key {template:?} references unknown identifier {var:?}")]
    UnknownTemplateVar {
        /// Step that triggered the failure.
        step_id: String,
        /// The raw template string.
        template: String,
        /// The unresolved root identifier.
        var: String,
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

/// Errors returned by [`crate::WorkflowExecutor::resume`] (issue #565).
#[derive(Debug, Error)]
pub enum WorkflowResumeError {
    /// No row found in `workflows` for this id.
    #[error("workflow {0} not found in storage")]
    NotFound(uuid::Uuid),

    /// The row exists but is in a state where resume makes no sense
    /// (e.g. `Completed`, `Cancelled`, or already `Running` in another
    /// process).
    #[error(
        "workflow {workflow_id} is in state {status}; only Failed / Interrupted / Pending workflows can be resumed"
    )]
    NotResumable {
        /// Workflow id under inspection.
        workflow_id: uuid::Uuid,
        /// Stringified current status.
        status: String,
    },

    /// The persisted spec hash differs from the caller-supplied
    /// `expected_spec_hash` and `strict=true` was requested.
    #[error("spec drift detected for workflow {workflow_id}: expected {expected}, found {actual}")]
    SpecChanged {
        /// Workflow id under inspection.
        workflow_id: uuid::Uuid,
        /// Hash the caller asserted.
        expected: String,
        /// Hash actually persisted on the row.
        actual: String,
    },

    /// Persistence is not configured on the executor (no
    /// `WorkflowStorage` was supplied at build time).
    #[error("resume requires a WorkflowStorage configured on the executor")]
    NoStorage,

    /// The persisted spec failed to deserialise. Indicates corruption
    /// or a forward-incompatible schema change; the caller should
    /// re-run the workflow from scratch.
    #[error("failed to deserialise persisted spec for workflow {workflow_id}: {reason}")]
    CorruptSpec {
        /// Workflow id under inspection.
        workflow_id: uuid::Uuid,
        /// Underlying parse error message.
        reason: String,
    },

    /// The re-validated spec failed validation (e.g. references a tool
    /// that has since been removed from the registry).
    #[error("spec for workflow {1} failed re-validation: {0}")]
    Validation(#[source] ValidationError, uuid::Uuid),

    /// Underlying storage layer I/O error.
    #[cfg(feature = "job-persist-sqlite")]
    #[error("storage error: {0}")]
    Storage(#[from] crate::sqlite::WorkflowStorageError),
}
