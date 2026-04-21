//! PyO3 bindings for the workflow crate (minimal skeleton surface).
//!
//! Only `WorkflowSpec` and `WorkflowStatus` are exposed. Step execution is
//! deferred; there is no `WorkflowJob` Python class yet.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;

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

    fn __repr__(&self) -> String {
        format!(
            "WorkflowSpec(name={:?}, steps={})",
            self.inner.name,
            self.inner.steps.len()
        )
    }
}

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
    Ok(())
}
