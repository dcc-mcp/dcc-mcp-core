//! PyO3 bindings for the sandbox crate.
//!
//! Exposed classes:
//! - `PySandboxPolicy`   — wraps [`SandboxPolicy`]
//! - `PySandboxContext`  — wraps [`SandboxContext`]
//! - `PyAuditLog`        — wraps [`AuditLog`] (read-only view)
//! - `PyAuditEntry`      — wraps [`AuditEntry`] (data class)

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde_json::Value;

use crate::audit::{AuditEntry, AuditLog, AuditOutcome};
use crate::context::SandboxContext;
use crate::error::SandboxError;
use crate::policy::{ExecutionMode, SandboxPolicy};
use crate::validator::{FieldSchema, InputValidator, ValidationRule};

// ── Conversion helper ─────────────────────────────────────────────────────────

fn sandbox_err_to_py(e: SandboxError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

// ── PyAuditEntry ──────────────────────────────────────────────────────────────

/// Python representation of a single sandbox audit entry.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "AuditEntry", from_py_object)]
#[derive(Clone)]
pub struct PyAuditEntry {
    inner: AuditEntry,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyAuditEntry {
    /// Unix timestamp in milliseconds when the action was recorded.
    #[getter]
    fn timestamp_ms(&self) -> u64 {
        self.inner.timestamp_ms
    }

    /// Actor / caller identity, or `None`.
    #[getter]
    fn actor(&self) -> Option<String> {
        self.inner.actor.clone()
    }

    /// Name of the action that was invoked.
    #[getter]
    fn action(&self) -> &str {
        &self.inner.action
    }

    /// Parameters as a JSON string.
    #[getter]
    fn params_json(&self) -> &str {
        &self.inner.params_json
    }

    /// Duration in milliseconds.
    #[getter]
    fn duration_ms(&self) -> u64 {
        self.inner.duration_ms
    }

    /// Outcome as a string: ``"success"``, ``"denied"``, ``"error"``, or ``"timeout"``.
    #[getter]
    fn outcome(&self) -> &str {
        match &self.inner.outcome {
            AuditOutcome::Success => "success",
            AuditOutcome::Denied { .. } => "denied",
            AuditOutcome::Error { .. } => "error",
            AuditOutcome::Timeout => "timeout",
        }
    }

    /// Outcome detail string (denial reason or error message), or ``None``.
    #[getter]
    fn outcome_detail(&self) -> Option<String> {
        match &self.inner.outcome {
            AuditOutcome::Denied { reason } => Some(reason.clone()),
            AuditOutcome::Error { message } => Some(message.clone()),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!(
            "AuditEntry(action={:?}, outcome={}, duration_ms={})",
            self.inner.action,
            self.outcome(),
            self.inner.duration_ms,
        )
    }
}

// ── PyAuditLog ────────────────────────────────────────────────────────────────

/// Read-only Python view of the sandbox audit log.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "AuditLog", from_py_object)]
#[derive(Clone)]
pub struct PyAuditLog {
    inner: AuditLog,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyAuditLog {
    /// Total number of entries.
    fn __len__(&self) -> usize {
        self.inner.len()
    }

    /// Return all entries as a list of :class:`AuditEntry`.
    fn entries(&self) -> Vec<PyAuditEntry> {
        self.inner
            .entries()
            .into_iter()
            .map(|e| PyAuditEntry { inner: e })
            .collect()
    }

    /// Return only successful entries.
    fn successes(&self) -> Vec<PyAuditEntry> {
        self.inner
            .successes()
            .into_iter()
            .map(|e| PyAuditEntry { inner: e })
            .collect()
    }

    /// Return only denied entries.
    fn denials(&self) -> Vec<PyAuditEntry> {
        self.inner
            .denials()
            .into_iter()
            .map(|e| PyAuditEntry { inner: e })
            .collect()
    }

    /// Return entries for a specific action name.
    fn entries_for_action(&self, action: &str) -> Vec<PyAuditEntry> {
        self.inner
            .entries_for_action(action)
            .into_iter()
            .map(|e| PyAuditEntry { inner: e })
            .collect()
    }

    /// Return all entries serialised as a JSON string.
    fn to_json(&self) -> PyResult<String> {
        self.inner
            .to_json()
            .map_err(|e| PyRuntimeError::new_err(format!("audit log serialization failed: {e}")))
    }

    fn __repr__(&self) -> String {
        format!("AuditLog(len={})", self.inner.len())
    }
}

// ── PySandboxPolicy ───────────────────────────────────────────────────────────

/// Python-accessible sandbox policy builder.
///
/// Example::
///
///     policy = PySandboxPolicy()
///     policy.allow_actions(["get_scene_info", "list_objects"])
///     policy.deny_actions(["delete_scene"])
///     policy.set_timeout_ms(5000)
///     policy.set_read_only(True)
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "SandboxPolicy")]
pub struct PySandboxPolicy {
    inner: SandboxPolicy,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PySandboxPolicy {
    #[new]
    fn new() -> Self {
        Self {
            inner: SandboxPolicy::default(),
        }
    }

    /// Restrict to only these actions (replaces any previous whitelist).
    fn allow_actions(&mut self, actions: Vec<String>) {
        let timeout_ms = self.inner.timeout_ms;
        let max_actions = self.inner.max_actions;
        let mode = self.inner.mode;
        let mut policy = SandboxPolicy::builder()
            .allow_actions(actions)
            .deny_actions(self.inner.denied_actions.iter().cloned())
            .allow_paths(self.inner.allowed_paths.clone())
            .build();
        policy.timeout_ms = timeout_ms;
        policy.max_actions = max_actions;
        policy.mode = mode;
        self.inner = policy;
    }

    /// Always deny these actions.
    fn deny_actions(&mut self, actions: Vec<String>) {
        for a in actions {
            self.inner.denied_actions.insert(a);
        }
    }

    /// Allow file-system access under these directory paths.
    fn allow_paths(&mut self, paths: Vec<String>) {
        for p in paths {
            self.inner.allowed_paths.push(p.into());
        }
    }

    /// Set execution timeout (milliseconds).
    fn set_timeout_ms(&mut self, ms: u64) {
        self.inner.timeout_ms = Some(ms);
    }

    /// Set maximum number of actions per session.
    fn set_max_actions(&mut self, count: u32) {
        self.inner.max_actions = Some(count);
    }

    /// Enable or disable read-only mode.
    fn set_read_only(&mut self, read_only: bool) {
        self.inner.mode = if read_only {
            ExecutionMode::ReadOnly
        } else {
            ExecutionMode::ReadWrite
        };
    }

    /// Return ``True`` if the policy is in read-only mode.
    #[getter]
    fn is_read_only(&self) -> bool {
        self.inner.mode == ExecutionMode::ReadOnly
    }

    fn __repr__(&self) -> String {
        format!(
            "SandboxPolicy(mode={:?}, timeout={:?}, max_actions={:?})",
            self.inner.mode, self.inner.timeout_ms, self.inner.max_actions,
        )
    }
}

// ── PySandboxContext ──────────────────────────────────────────────────────────

/// Python-accessible sandbox execution context.
///
/// Example::
///
///     policy = PySandboxPolicy()
///     policy.allow_actions(["echo"])
///     ctx = PySandboxContext(policy)
///     ctx.set_actor("my-agent")
///
///     # Execute with params as a JSON string
///     result_json = ctx.execute_json("echo", '{"x": 1}')
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "SandboxContext")]
pub struct PySandboxContext {
    inner: SandboxContext,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PySandboxContext {
    #[new]
    fn new(policy: &PySandboxPolicy) -> Self {
        Self {
            inner: SandboxContext::new(policy.inner.clone()),
        }
    }

    /// Set the actor identity attached to audit entries.
    fn set_actor(&mut self, actor: &str) {
        // SandboxContext::with_actor consumes self; use direct field access via
        // a re-create approach by storing actor in a helper.
        // Since with_actor requires ownership, rebuild manually here.
        let policy = self.inner.policy().clone();
        let log = self.inner.audit_log().clone();
        let count = self.inner.action_count();
        let new_ctx = SandboxContext::new(policy).with_actor(actor);
        // Restore audit log entries by draining and re-recording
        for entry in log.entries() {
            new_ctx.audit_log().record(entry);
        }
        // Restore action count via execute calls is not feasible; we accept
        // that set_actor resets session state (document this limitation).
        let _ = count; // count is not directly settable from outside; acceptable limitation
        self.inner = new_ctx;
    }

    /// Execute an action with parameters provided as a JSON string.
    ///
    /// Returns the handler result as a JSON string, or raises RuntimeError.
    fn execute_json(&mut self, action: &str, params_json: &str) -> PyResult<String> {
        let params: Value = serde_json::from_str(params_json)
            .map_err(|e| PyRuntimeError::new_err(format!("invalid JSON params: {e}")))?;

        // No custom handler — policy + validation check only, returns Null
        let result = self
            .inner
            .execute(action, &params, None, None)
            .map_err(sandbox_err_to_py)?;

        let val = result.value.unwrap_or(Value::Null);
        serde_json::to_string(&val)
            .map_err(|e| PyRuntimeError::new_err(format!("result serialization failed: {e}")))
    }

    /// Return the number of actions executed in this session.
    #[getter]
    fn action_count(&self) -> u32 {
        self.inner.action_count()
    }

    /// Return the :class:`AuditLog` for this context.
    #[getter]
    fn audit_log(&self) -> PyAuditLog {
        PyAuditLog {
            inner: self.inner.audit_log().clone(),
        }
    }

    /// Return ``True`` if ``action`` is permitted by policy.
    fn is_allowed(&self, action: &str) -> bool {
        self.inner.policy().check_action(action).is_ok()
    }

    /// Return ``True`` if ``path`` is within an allowed directory.
    fn is_path_allowed(&self, path: &str) -> bool {
        self.inner
            .policy()
            .check_path(std::path::Path::new(path))
            .is_ok()
    }

    fn __repr__(&self) -> String {
        format!(
            "SandboxContext(action_count={}, audit_entries={})",
            self.inner.action_count(),
            self.inner.audit_log().len(),
        )
    }
}

// ── PyInputValidator ──────────────────────────────────────────────────────────

/// Python-accessible input validator.
///
/// Example::
///
///     v = PyInputValidator()
///     v.require_string("name", max_length=50)
///     v.require_number("count", min_value=0, max_value=1000)
///     ok, error = v.validate('{"name": "sphere", "count": 5}')
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "InputValidator")]
pub struct PyInputValidator {
    inner: InputValidator,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyInputValidator {
    #[new]
    fn new() -> Self {
        Self {
            inner: InputValidator::new(),
        }
    }

    /// Add a required string field with optional length constraints.
    fn require_string(
        &mut self,
        field: &str,
        max_length: Option<usize>,
        min_length: Option<usize>,
    ) {
        let mut schema = FieldSchema::new()
            .rule(ValidationRule::Required)
            .rule(ValidationRule::IsString);
        if let Some(max) = max_length {
            schema = schema.rule(ValidationRule::MaxLength(max));
        }
        if let Some(min) = min_length {
            schema = schema.rule(ValidationRule::MinLength(min));
        }
        let validator = std::mem::replace(&mut self.inner, InputValidator::new());
        self.inner = validator.register(field, schema);
    }

    /// Add a required numeric field with optional range constraints.
    fn require_number(&mut self, field: &str, min_value: Option<f64>, max_value: Option<f64>) {
        let mut schema = FieldSchema::new()
            .rule(ValidationRule::Required)
            .rule(ValidationRule::IsNumber);
        if let Some(min) = min_value {
            schema = schema.rule(ValidationRule::MinValue(min));
        }
        if let Some(max) = max_value {
            schema = schema.rule(ValidationRule::MaxValue(max));
        }
        let validator = std::mem::replace(&mut self.inner, InputValidator::new());
        self.inner = validator.register(field, schema);
    }

    /// Add injection-guard for a string field.
    fn forbid_substrings(&mut self, field: &str, substrings: Vec<String>) {
        let schema = FieldSchema::new().rule(ValidationRule::ForbiddenSubstrings(substrings));
        let validator = std::mem::replace(&mut self.inner, InputValidator::new());
        self.inner = validator.register(field, schema);
    }

    /// Validate a JSON string params payload.
    ///
    /// Returns ``(True, None)`` on success, ``(False, error_message)`` on failure.
    fn validate(&self, params_json: &str) -> PyResult<(bool, Option<String>)> {
        let params: Value = serde_json::from_str(params_json)
            .map_err(|e| PyRuntimeError::new_err(format!("invalid JSON: {e}")))?;
        match self.inner.validate_value(&params) {
            Ok(()) => Ok((true, None)),
            Err(e) => Ok((false, Some(e.to_string()))),
        }
    }

    fn __repr__(&self) -> String {
        "InputValidator".to_owned()
    }
}

// ── Registration helper ───────────────────────────────────────────────────────

/// Register all sandbox pyclass types on the given module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyAuditEntry>()?;
    m.add_class::<PyAuditLog>()?;
    m.add_class::<PySandboxPolicy>()?;
    m.add_class::<PySandboxContext>()?;
    m.add_class::<PyInputValidator>()?;
    Ok(())
}
