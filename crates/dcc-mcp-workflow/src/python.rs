//! PyO3 bindings for the workflow crate (minimal skeleton surface).
//!
//! Only `WorkflowSpec` and `WorkflowStatus` are exposed. Step execution is
//! deferred; there is no `WorkflowJob` Python class yet.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

use crate::policy::{BackoffKind, IdempotencyScope, RetryPolicy, StepPolicy};
use crate::spec::{WorkflowSpec, WorkflowStatus};

/// Python wrapper for [`crate::WorkflowSpec`].
///
/// Only two operations are exposed in this skeleton:
/// - classmethod `from_yaml_str(s: str) -> PyWorkflowSpec`
/// - instance method `validate() -> None` (raises `ValueError` on failure)
#[pyclass(
    name = "WorkflowSpec",
    module = "dcc_mcp_core._core",
    skip_from_py_object
)]
#[derive(Debug, Clone)]
pub struct PyWorkflowSpec {
    inner: WorkflowSpec,
}

#[pymethods]
impl PyWorkflowSpec {
    /// Parse a workflow spec from a YAML string.
    #[classmethod]
    fn from_yaml_str(_cls: &Bound<'_, pyo3::types::PyType>, source: &str) -> PyResult<Self> {
        WorkflowSpec::from_yaml(source)
            .map(|inner| Self { inner })
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Validate the spec. Raises ``ValueError`` on failure.
    fn validate(&self) -> PyResult<()> {
        self.inner
            .validate()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Workflow name.
    #[getter]
    fn name(&self) -> &str {
        &self.inner.name
    }

    /// Human-readable description.
    #[getter]
    fn description(&self) -> &str {
        &self.inner.description
    }

    /// Number of top-level steps.
    #[getter]
    fn step_count(&self) -> usize {
        self.inner.steps.len()
    }

    /// Serialise back to YAML (round-trippable with `from_yaml_str`).
    fn to_yaml(&self) -> PyResult<String> {
        serde_yaml_ng::to_string(&self.inner).map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Access the full list of top-level steps as Python wrappers.
    ///
    /// Each returned [`PyWorkflowStep`] is a **snapshot** — mutations
    /// through Python do not flow back into this spec.
    #[getter]
    fn steps(&self) -> Vec<PyWorkflowStep> {
        self.inner
            .steps
            .iter()
            .map(|s| PyWorkflowStep {
                id: s.id.0.clone(),
                kind: s.kind.kind_str(),
                policy: s.policy.clone(),
            })
            .collect()
    }

    fn __repr__(&self) -> String {
        format!(
            "WorkflowSpec(name={:?}, steps={})",
            self.inner.name,
            self.inner.steps.len()
        )
    }
}

// ── Step / Policy wrappers ───────────────────────────────────────────────

/// Read-only Python view of a single [`crate::Step`].
#[pyclass(
    name = "WorkflowStep",
    module = "dcc_mcp_core._core",
    skip_from_py_object,
    frozen
)]
#[derive(Debug, Clone)]
pub struct PyWorkflowStep {
    id: String,
    kind: &'static str,
    policy: StepPolicy,
}

#[pymethods]
impl PyWorkflowStep {
    /// Declared step id.
    #[getter]
    fn id(&self) -> &str {
        &self.id
    }

    /// Kind tag: `"tool"`, `"tool_remote"`, `"foreach"`, `"parallel"`,
    /// `"approve"`, `"branch"`.
    #[getter]
    fn kind(&self) -> &'static str {
        self.kind
    }

    /// Per-step execution policy snapshot.
    #[getter]
    fn policy(&self) -> PyStepPolicy {
        PyStepPolicy {
            inner: self.policy.clone(),
        }
    }

    fn __repr__(&self) -> String {
        format!("WorkflowStep(id={:?}, kind={:?})", self.id, self.kind)
    }
}

/// Read-only Python view of a [`StepPolicy`].
#[pyclass(
    name = "StepPolicy",
    module = "dcc_mcp_core._core",
    skip_from_py_object,
    frozen
)]
#[derive(Debug, Clone)]
pub struct PyStepPolicy {
    inner: StepPolicy,
}

#[pymethods]
impl PyStepPolicy {
    /// Absolute wall-clock timeout in seconds. ``None`` = no timeout.
    #[getter]
    fn timeout_secs(&self) -> Option<u64> {
        self.inner.timeout.map(|d| d.as_secs())
    }

    /// Retry policy. ``None`` = single attempt, no retry.
    #[getter]
    fn retry(&self) -> Option<PyRetryPolicy> {
        self.inner
            .retry
            .as_ref()
            .map(|r| PyRetryPolicy { inner: r.clone() })
    }

    /// Raw idempotency-key template. ``None`` when unset.
    #[getter]
    fn idempotency_key(&self) -> Option<&str> {
        self.inner.idempotency_key.as_deref()
    }

    /// Idempotency scope — one of ``"workflow"`` (default) or ``"global"``.
    #[getter]
    fn idempotency_scope(&self) -> &'static str {
        self.inner.idempotency_scope.as_str()
    }

    /// Whether every knob is at its default.
    #[getter]
    fn is_empty(&self) -> bool {
        self.inner.is_empty()
    }

    fn __repr__(&self) -> String {
        format!(
            "StepPolicy(timeout_secs={:?}, retry={}, idempotency_key={:?})",
            self.inner.timeout.map(|d| d.as_secs()),
            if self.inner.retry.is_some() {
                "Some"
            } else {
                "None"
            },
            self.inner.idempotency_key
        )
    }
}

/// Read-only Python view of a [`RetryPolicy`].
#[pyclass(
    name = "RetryPolicy",
    module = "dcc_mcp_core._core",
    skip_from_py_object,
    frozen
)]
#[derive(Debug, Clone)]
pub struct PyRetryPolicy {
    inner: RetryPolicy,
}

#[pymethods]
impl PyRetryPolicy {
    /// Cap on the number of **attempts** (1 = no retry).
    #[getter]
    fn max_attempts(&self) -> u32 {
        self.inner.max_attempts
    }

    /// Backoff shape: ``"fixed"``, ``"linear"``, or ``"exponential"``.
    #[getter]
    fn backoff(&self) -> &'static str {
        self.inner.backoff.as_str()
    }

    /// Base delay in milliseconds.
    #[getter]
    fn initial_delay_ms(&self) -> u64 {
        self.inner.initial_delay.as_millis() as u64
    }

    /// Upper delay bound in milliseconds.
    #[getter]
    fn max_delay_ms(&self) -> u64 {
        self.inner.max_delay.as_millis() as u64
    }

    /// Relative jitter in ``[0.0, 1.0]``.
    #[getter]
    fn jitter(&self) -> f32 {
        self.inner.jitter
    }

    /// Optional error-kind allowlist. ``None`` = every error is
    /// retryable.
    #[getter]
    fn retry_on(&self) -> Option<Vec<String>> {
        self.inner.retry_on.clone()
    }

    /// Compute the **base** backoff for a 1-indexed attempt number.
    /// Matches [`RetryPolicy::next_delay`] on the Rust side.
    fn next_delay_ms(&self, attempt_number: u32) -> u64 {
        self.inner.next_delay(attempt_number).as_millis() as u64
    }

    /// Whether this policy considers the given error kind retryable.
    fn is_retryable(&self, error_kind: &str) -> bool {
        self.inner.is_retryable(error_kind)
    }

    fn __repr__(&self) -> String {
        format!(
            "RetryPolicy(max_attempts={}, backoff={:?}, initial_delay_ms={}, max_delay_ms={}, jitter={})",
            self.inner.max_attempts,
            self.inner.backoff.as_str(),
            self.inner.initial_delay.as_millis(),
            self.inner.max_delay.as_millis(),
            self.inner.jitter,
        )
    }
}

/// String constants for [`BackoffKind`] / [`IdempotencyScope`] — exposed
/// so Python callers can compare against the policy getters.
#[pyclass(
    name = "BackoffKind",
    module = "dcc_mcp_core._core",
    skip_from_py_object,
    frozen
)]
#[derive(Debug, Clone, Copy)]
pub struct PyBackoffKind;

#[pymethods]
impl PyBackoffKind {
    /// ``"fixed"``
    #[classattr]
    const FIXED: &'static str = BackoffKind::Fixed.as_str();
    /// ``"linear"``
    #[classattr]
    const LINEAR: &'static str = BackoffKind::Linear.as_str();
    /// ``"exponential"``
    #[classattr]
    const EXPONENTIAL: &'static str = BackoffKind::Exponential.as_str();

    /// Every valid backoff kind as a tuple of strings.
    #[classattr]
    const VALUES: (&'static str, &'static str, &'static str) = ("fixed", "linear", "exponential");
}

// Silence unused-import warning for IdempotencyScope when we don't emit a
// Python class for it — the string constants on PyStepPolicy suffice.
#[allow(dead_code)]
const _SCOPE_WORKFLOW: &str = IdempotencyScope::Workflow.as_str();

/// Python wrapper for [`crate::WorkflowStatus`].
///
/// Exposed as string constants via classmethods to keep pyo3 surface flat.
#[pyclass(
    name = "WorkflowStatus",
    module = "dcc_mcp_core._core",
    skip_from_py_object
)]
#[derive(Debug, Clone, Copy)]
pub struct PyWorkflowStatus {
    inner: WorkflowStatus,
}

#[pymethods]
impl PyWorkflowStatus {
    /// Construct from one of: `"pending"`, `"running"`, `"completed"`,
    /// `"failed"`, `"cancelled"`, `"interrupted"`.
    #[new]
    fn new(value: &str) -> PyResult<Self> {
        let inner = match value {
            "pending" => WorkflowStatus::Pending,
            "running" => WorkflowStatus::Running,
            "completed" => WorkflowStatus::Completed,
            "failed" => WorkflowStatus::Failed,
            "cancelled" => WorkflowStatus::Cancelled,
            "interrupted" => WorkflowStatus::Interrupted,
            other => {
                return Err(PyValueError::new_err(format!(
                    "unknown workflow status: {other:?}"
                )));
            }
        };
        Ok(Self { inner })
    }

    /// Whether this status represents a terminal state.
    #[getter]
    fn is_terminal(&self) -> bool {
        self.inner.is_terminal()
    }

    /// Lowercase string representation.
    #[getter]
    fn value(&self) -> &'static str {
        self.inner.as_str()
    }

    fn __repr__(&self) -> String {
        format!("WorkflowStatus({:?})", self.inner.as_str())
    }

    fn __str__(&self) -> &'static str {
        self.inner.as_str()
    }
}

/// Register the workflow classes on a Python module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyWorkflowSpec>()?;
    m.add_class::<PyWorkflowStatus>()?;
    m.add_class::<PyWorkflowStep>()?;
    m.add_class::<PyStepPolicy>()?;
    m.add_class::<PyRetryPolicy>()?;
    m.add_class::<PyBackoffKind>()?;
    Ok(())
}
