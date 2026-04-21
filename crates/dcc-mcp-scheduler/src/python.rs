//! PyO3 bindings for the scheduler crate.
//!
//! Only the declarative types are exposed — the runtime
//! ([`SchedulerService`](crate::SchedulerService)) is driven from Rust
//! inside the McpHttpServer. Python code can:
//!
//! * Parse and validate a `*.schedules.yaml` file (via [`PyScheduleFile`]).
//! * Build / inspect individual [`PyScheduleSpec`] / [`PyTriggerSpec`] entries.
//! * Compute and verify HMAC signatures for webhook testing.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyType;

use crate::spec::{ScheduleFile, ScheduleSpec, TriggerSpec};
use crate::webhook::{compute_signature, verify_hub_signature_256};

/// Python wrapper for [`TriggerSpec`].
#[pyclass(name = "TriggerSpec", module = "dcc_mcp_core._core", from_py_object)]
#[derive(Debug, Clone)]
pub struct PyTriggerSpec {
    inner: TriggerSpec,
}

#[pymethods]
impl PyTriggerSpec {
    /// Build a cron trigger.
    #[classmethod]
    #[pyo3(signature = (expression, timezone="UTC", jitter_secs=0))]
    fn cron(
        _cls: &Bound<'_, PyType>,
        expression: &str,
        timezone: &str,
        jitter_secs: u32,
    ) -> PyResult<Self> {
        crate::service::parse_cron(expression).map_err(|e| PyValueError::new_err(e.to_string()))?;
        crate::service::parse_timezone(timezone)
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self {
            inner: TriggerSpec::Cron {
                expression: expression.to_string(),
                timezone: timezone.to_string(),
                jitter_secs,
            },
        })
    }

    /// Build a webhook trigger.
    #[classmethod]
    #[pyo3(signature = (path, secret_env=None))]
    fn webhook(_cls: &Bound<'_, PyType>, path: &str, secret_env: Option<String>) -> PyResult<Self> {
        if !path.starts_with('/') {
            return Err(PyValueError::new_err("webhook path must start with '/'"));
        }
        Ok(Self {
            inner: TriggerSpec::Webhook {
                path: path.to_string(),
                secret_env,
            },
        })
    }

    /// Trigger kind — `"cron"` or `"webhook"`.
    #[getter]
    fn kind(&self) -> &'static str {
        match self.inner {
            TriggerSpec::Cron { .. } => "cron",
            TriggerSpec::Webhook { .. } => "webhook",
        }
    }

    /// Cron expression (None for webhook triggers).
    #[getter]
    fn expression(&self) -> Option<String> {
        match &self.inner {
            TriggerSpec::Cron { expression, .. } => Some(expression.clone()),
            _ => None,
        }
    }

    /// Timezone name (None for webhook triggers).
    #[getter]
    fn timezone(&self) -> Option<String> {
        match &self.inner {
            TriggerSpec::Cron { timezone, .. } => Some(timezone.clone()),
            _ => None,
        }
    }

    /// Jitter (seconds, 0 for webhook).
    #[getter]
    fn jitter_secs(&self) -> u32 {
        match &self.inner {
            TriggerSpec::Cron { jitter_secs, .. } => *jitter_secs,
            _ => 0,
        }
    }

    /// Webhook path (None for cron triggers).
    #[getter]
    fn path(&self) -> Option<String> {
        match &self.inner {
            TriggerSpec::Webhook { path, .. } => Some(path.clone()),
            _ => None,
        }
    }

    /// Webhook secret env var name (None for cron triggers or no-secret webhooks).
    #[getter]
    fn secret_env(&self) -> Option<String> {
        match &self.inner {
            TriggerSpec::Webhook { secret_env, .. } => secret_env.clone(),
            _ => None,
        }
    }

    fn __repr__(&self) -> String {
        format!("TriggerSpec(kind={:?})", self.kind())
    }
}

/// Python wrapper for [`ScheduleSpec`].
#[pyclass(
    name = "ScheduleSpec",
    module = "dcc_mcp_core._core",
    skip_from_py_object
)]
#[derive(Debug, Clone)]
pub struct PyScheduleSpec {
    inner: ScheduleSpec,
}

#[pymethods]
impl PyScheduleSpec {
    /// Construct a schedule spec directly.
    #[new]
    #[pyo3(signature = (id, workflow, trigger, inputs=None, enabled=true, max_concurrent=1))]
    fn new(
        id: String,
        workflow: String,
        trigger: PyTriggerSpec,
        inputs: Option<&str>,
        enabled: bool,
        max_concurrent: u32,
    ) -> PyResult<Self> {
        let inputs_val = match inputs {
            Some(s) => serde_json::from_str(s)
                .map_err(|e| PyValueError::new_err(format!("invalid JSON for inputs: {e}")))?,
            None => serde_json::Value::Null,
        };
        let spec = ScheduleSpec {
            id,
            workflow,
            inputs: inputs_val,
            trigger: trigger.inner,
            enabled,
            max_concurrent,
        };
        spec.validate()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
        Ok(Self { inner: spec })
    }

    /// Validate the spec (raises `ValueError` on failure).
    fn validate(&self) -> PyResult<()> {
        self.inner
            .validate()
            .map_err(|e| PyValueError::new_err(e.to_string()))
    }

    /// Schedule id.
    #[getter]
    fn id(&self) -> &str {
        &self.inner.id
    }

    /// Workflow name.
    #[getter]
    fn workflow(&self) -> &str {
        &self.inner.workflow
    }

    /// Workflow inputs rendered as a JSON string (before placeholder substitution).
    #[getter]
    fn inputs_json(&self) -> String {
        self.inner.inputs.to_string()
    }

    /// Schedule trigger.
    #[getter]
    fn trigger(&self) -> PyTriggerSpec {
        PyTriggerSpec {
            inner: self.inner.trigger.clone(),
        }
    }

    /// Whether the schedule is currently enabled.
    #[getter]
    fn enabled(&self) -> bool {
        self.inner.enabled
    }

    /// Maximum concurrent in-flight fires (0 = unlimited).
    #[getter]
    fn max_concurrent(&self) -> u32 {
        self.inner.max_concurrent
    }

    fn __repr__(&self) -> String {
        format!(
            "ScheduleSpec(id={:?}, workflow={:?}, trigger_kind={:?})",
            self.inner.id,
            self.inner.workflow,
            match self.inner.trigger {
                TriggerSpec::Cron { .. } => "cron",
                TriggerSpec::Webhook { .. } => "webhook",
            }
        )
    }
}

/// Parse a `*.schedules.yaml` document. Returns a list of
/// [`PyScheduleSpec`].
///
/// # Errors
///
/// Raises `ValueError` on parse or validation failure.
#[pyfunction]
#[pyo3(name = "parse_schedules_yaml", signature = (source, path_hint="<string>".to_string()))]
pub fn py_parse_schedules_yaml(source: &str, path_hint: String) -> PyResult<Vec<PyScheduleSpec>> {
    let file = ScheduleFile::from_yaml_str(source, &path_hint)
        .map_err(|e| PyValueError::new_err(e.to_string()))?;
    for s in &file.schedules {
        s.validate()
            .map_err(|e| PyValueError::new_err(e.to_string()))?;
    }
    Ok(file
        .schedules
        .into_iter()
        .map(|s| PyScheduleSpec { inner: s })
        .collect())
}

/// Compute the canonical `sha256=<hex>` HMAC signature for `body` under
/// `secret`. Exposed for webhook-sender testing.
#[pyfunction]
#[pyo3(name = "hmac_sha256_hex")]
pub fn py_hmac_sha256_hex(secret: &[u8], body: &[u8]) -> String {
    compute_signature(secret, body)
}

/// Verify a `X-Hub-Signature-256` header value.
#[pyfunction]
#[pyo3(name = "verify_hub_signature_256", signature = (secret, body, header_value))]
pub fn py_verify_hub_signature_256(secret: &[u8], body: &[u8], header_value: Option<&str>) -> bool {
    verify_hub_signature_256(secret, body, header_value)
}

/// Register the scheduler Python classes on the extension module.
///
/// # Errors
///
/// Propagates PyO3 registration failures.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyScheduleSpec>()?;
    m.add_class::<PyTriggerSpec>()?;
    m.add_function(wrap_pyfunction!(py_parse_schedules_yaml, m)?)?;
    m.add_function(wrap_pyfunction!(py_hmac_sha256_hex, m)?)?;
    m.add_function(wrap_pyfunction!(py_verify_hub_signature_256, m)?)?;
    Ok(())
}
