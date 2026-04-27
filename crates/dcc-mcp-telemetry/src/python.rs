//! PyO3 Python bindings for dcc-mcp-telemetry.
//!
//! Exposed classes and functions:
//!
//! | Python name | Rust type | Purpose |
//! |-------------|-----------|---------|
//! | `TelemetryConfig` | [`PyTelemetryConfig`] | Build and apply telemetry configuration |
//! | `ToolRecorder` | [`PyActionRecorder`] | Record per-tool metrics |
//! | `ToolMetrics` | [`PyActionMetrics`] | Read-only metrics snapshot |

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pyfunction, gen_stub_pymethods};

use crate::provider;
use crate::recorder::ActionRecorder;
use crate::types::{ActionMetrics, ExporterBackend, LogFormat, TelemetryConfig};

// ── PyActionMetrics ───────────────────────────────────────────────────────────

/// Read-only snapshot of per-Action performance metrics.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ToolMetrics", from_py_object)]
#[derive(Clone)]
pub struct PyActionMetrics {
    inner: ActionMetrics,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyActionMetrics {
    /// Action name.
    #[getter]
    pub fn action_name(&self) -> &str {
        &self.inner.action_name
    }

    /// Total number of invocations.
    #[getter]
    pub fn invocation_count(&self) -> u64 {
        self.inner.invocation_count
    }

    /// Number of successful invocations.
    #[getter]
    pub fn success_count(&self) -> u64 {
        self.inner.success_count
    }

    /// Number of failed invocations.
    #[getter]
    pub fn failure_count(&self) -> u64 {
        self.inner.failure_count
    }

    /// Average execution duration in milliseconds.
    #[getter]
    pub fn avg_duration_ms(&self) -> f64 {
        self.inner.avg_duration_ms
    }

    /// P95 execution duration in milliseconds.
    #[getter]
    pub fn p95_duration_ms(&self) -> f64 {
        self.inner.p95_duration_ms
    }

    /// P99 execution duration in milliseconds.
    #[getter]
    pub fn p99_duration_ms(&self) -> f64 {
        self.inner.p99_duration_ms
    }

    /// Success rate as a fraction in [0.0, 1.0].
    pub fn success_rate(&self) -> f64 {
        self.inner.success_rate()
    }

    fn __repr__(&self) -> String {
        format!(
            "ToolMetrics(tool={:?}, invocations={}, success_rate={:.2})",
            self.inner.action_name,
            self.inner.invocation_count,
            self.inner.success_rate()
        )
    }
}

// ── PyTelemetryConfig ─────────────────────────────────────────────────────────

/// Builder and initialiser for the global telemetry provider.
///
/// # Example
///
/// ```python
/// from dcc_mcp_core import TelemetryConfig
///
/// cfg = (TelemetryConfig("my-service")
///         .with_stdout_exporter()
///         .with_attribute("dcc.name", "maya"))
/// cfg.init()   # install global provider
/// cfg.shutdown()  # flush and close
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "TelemetryConfig")]
pub struct PyTelemetryConfig {
    inner: TelemetryConfig,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyTelemetryConfig {
    /// Create a new config for the given service name.
    #[new]
    pub fn new(service_name: String) -> Self {
        PyTelemetryConfig {
            inner: TelemetryConfig::builder(service_name).build(),
        }
    }

    /// Use the stdout exporter (prints spans/metrics to stdout).
    pub fn with_stdout_exporter(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.inner.exporter = ExporterBackend::Stdout;
        slf
    }

    /// Use the no-op exporter (discard all telemetry — useful in tests).
    pub fn with_noop_exporter(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.inner.exporter = ExporterBackend::Noop;
        slf
    }

    /// Use JSON log format.
    pub fn with_json_logs(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.inner.log_format = LogFormat::Json;
        slf
    }

    /// Use text log format (default).
    pub fn with_text_logs(mut slf: PyRefMut<'_, Self>) -> PyRefMut<'_, Self> {
        slf.inner.log_format = LogFormat::Text;
        slf
    }

    /// Add an extra resource attribute.
    pub fn with_attribute(
        mut slf: PyRefMut<'_, Self>,
        key: String,
        value: String,
    ) -> PyRefMut<'_, Self> {
        slf.inner.extra_attributes.insert(key, value);
        slf
    }

    /// Set the service version string.
    pub fn with_service_version(
        mut slf: PyRefMut<'_, Self>,
        version: String,
    ) -> PyRefMut<'_, Self> {
        slf.inner.service_version = version;
        slf
    }

    /// Enable or disable metrics collection.
    pub fn set_enable_metrics(mut slf: PyRefMut<'_, Self>, enabled: bool) -> PyRefMut<'_, Self> {
        slf.inner.enable_metrics = enabled;
        slf
    }

    /// Enable or disable distributed tracing.
    pub fn set_enable_tracing(mut slf: PyRefMut<'_, Self>, enabled: bool) -> PyRefMut<'_, Self> {
        slf.inner.enable_tracing = enabled;
        slf
    }

    /// Install this configuration as the global telemetry provider.
    ///
    /// Raises `RuntimeError` if a provider is already installed.
    pub fn init(&self) -> PyResult<()> {
        provider::init(&self.inner).map_err(|e| PyRuntimeError::new_err(e.to_string()))
    }

    /// Return the service name.
    #[getter]
    pub fn service_name(&self) -> &str {
        &self.inner.service_name
    }

    /// Return whether metrics are enabled.
    #[getter]
    pub fn enable_metrics(&self) -> bool {
        self.inner.enable_metrics
    }

    /// Return whether tracing is enabled.
    #[getter]
    pub fn enable_tracing(&self) -> bool {
        self.inner.enable_tracing
    }

    fn __repr__(&self) -> String {
        format!(
            "TelemetryConfig(service={:?}, exporter={:?})",
            self.inner.service_name, self.inner.exporter
        )
    }
}

// ── PyActionRecorder ──────────────────────────────────────────────────────────

/// Records per-Action execution time and success/failure counters.
///
/// # Example
///
/// ```python
/// from dcc_mcp_core import ActionRecorder
///
/// recorder = ActionRecorder("my-scope")
///
/// guard = recorder.start("create_sphere", "maya")
/// # ... do work ...
/// guard.finish(success=True)
///
/// metrics = recorder.metrics("create_sphere")
/// print(metrics.invocation_count, metrics.success_rate())
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ToolRecorder")]
pub struct PyActionRecorder {
    inner: ActionRecorder,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyActionRecorder {
    /// Create a new `ActionRecorder` for the given scope name.
    #[new]
    pub fn new(scope: &str) -> Self {
        // Safety: we need a 'static str for the recorder scope.
        // We leak it here — the number of scopes is small and bounded.
        let leaked: &'static str = Box::leak(scope.to_string().into_boxed_str());
        PyActionRecorder {
            inner: ActionRecorder::new(leaked),
        }
    }

    /// Start timing an action and return a guard object.
    pub fn start(&self, action_name: String, dcc_name: String) -> PyRecordingGuard {
        let guard = self.inner.start(&action_name, &dcc_name);
        PyRecordingGuard {
            guard: Some(guard),
            action_name,
            dcc_name,
        }
    }

    /// Get aggregated metrics for a specific action.
    ///
    /// Returns `None` if no data exists for this action.
    pub fn metrics(&self, action_name: &str) -> Option<PyActionMetrics> {
        self.inner
            .metrics(action_name)
            .map(|m| PyActionMetrics { inner: m })
    }

    /// Get aggregated metrics for all recorded actions.
    pub fn all_metrics(&self) -> Vec<PyActionMetrics> {
        self.inner
            .all_metrics()
            .into_iter()
            .map(|m| PyActionMetrics { inner: m })
            .collect()
    }

    /// Reset all in-memory statistics.
    pub fn reset(&self) {
        self.inner.reset();
    }
}

// ── PyRecordingGuard ──────────────────────────────────────────────────────────

/// Guard object returned by `ActionRecorder.start()`.
///
/// Call `finish(success)` to record the result, or let it drop to record as failure.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "RecordingGuard")]
pub struct PyRecordingGuard {
    guard: Option<crate::recorder::RecordingGuard>,
    action_name: String,
    dcc_name: String,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyRecordingGuard {
    /// Finish recording with the given success flag.
    pub fn finish(&mut self, success: bool) {
        if let Some(guard) = self.guard.take() {
            guard.finish(success);
        }
    }

    /// Context manager support — `__enter__` returns self.
    pub fn __enter__(slf: PyRef<'_, Self>) -> PyRef<'_, Self> {
        slf
    }

    /// Context manager support — `__exit__` calls `finish(success=True)`.
    pub fn __exit__(
        &mut self,
        exc_type: Option<&Bound<'_, PyAny>>,
        _exc_value: Option<&Bound<'_, PyAny>>,
        _traceback: Option<&Bound<'_, PyAny>>,
    ) {
        let success = exc_type.is_none();
        self.finish(success);
    }

    fn __repr__(&self) -> String {
        format!(
            "RecordingGuard(action={:?}, dcc={:?}, active={})",
            self.action_name,
            self.dcc_name,
            self.guard.is_some()
        )
    }
}

// ── Free functions ────────────────────────────────────────────────────────────

/// Return `True` if the global telemetry provider has been initialised.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "is_telemetry_initialized")]
pub fn py_is_telemetry_initialized() -> bool {
    provider::is_initialized()
}

/// Shut down the global telemetry provider, flushing all pending data.
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "shutdown_telemetry")]
pub fn py_shutdown_telemetry() {
    provider::shutdown();
}

/// Initialise a minimal no-op telemetry provider if one has not been set yet.
///
/// Silences the ``NoopMeterProvider`` warning that OpenTelemetry emits when
/// ``global::meter()`` is called before any provider has been registered
/// (issue #467).  Safe to call multiple times — a no-op when already
/// initialised.
///
/// Example::
///
///     from dcc_mcp_core import init_default_telemetry
///
///     # At server startup, before any MCP tools run:
///     init_default_telemetry()
///
/// Raises:
///     RuntimeError: If provider initialisation fails for a reason other than
///         "already initialized".
#[cfg_attr(feature = "stub-gen", gen_stub_pyfunction)]
#[pyfunction]
#[pyo3(name = "init_default_telemetry")]
pub fn py_init_default_telemetry() -> PyResult<()> {
    match provider::try_init_default() {
        Ok(()) => Ok(()),
        Err(crate::error::TelemetryError::AlreadyInitialized) => Ok(()),
        Err(e) => Err(PyRuntimeError::new_err(e.to_string())),
    }
}

// ── Registration ──────────────────────────────────────────────────────────────

/// Register all telemetry classes and functions on a Python module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyTelemetryConfig>()?;
    m.add_class::<PyActionRecorder>()?;
    m.add_class::<PyActionMetrics>()?;
    m.add_class::<PyRecordingGuard>()?;
    m.add_function(wrap_pyfunction!(py_is_telemetry_initialized, m)?)?;
    m.add_function(wrap_pyfunction!(py_shutdown_telemetry, m)?)?;
    m.add_function(wrap_pyfunction!(py_init_default_telemetry, m)?)?;
    Ok(())
}
