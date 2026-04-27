//! PyO3 wrappers for the four built-in middleware types:
//! [`PyLoggingMiddleware`], [`PyTimingMiddleware`], [`PyAuditMiddleware`],
//! [`PyRateLimitMiddleware`].
//!
//! Inner fields are `pub(super)` so the sibling `python_pipeline` module can
//! construct and return these wrappers from `PyActionPipeline::add_timing()`
//! etc.

use std::sync::Arc;
use std::time::Duration;

use pyo3::prelude::*;
use pyo3::types::PyDict;

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::pipeline::{AuditMiddleware, LoggingMiddleware, RateLimitMiddleware, TimingMiddleware};

// ── PyLoggingMiddleware ──────────────────────────────────────────────────────

/// Logging middleware — emits tracing log lines before/after each action.
///
/// ```python
/// from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline, LoggingMiddleware
///
/// reg = ActionRegistry()
/// dispatcher = ActionDispatcher(reg)
/// pipeline = ActionPipeline(dispatcher)
/// pipeline.add_logging(log_params=True)
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "LoggingMiddleware")]
pub struct PyLoggingMiddleware {
    pub(super) inner: LoggingMiddleware,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyLoggingMiddleware {
    /// Create a new logging middleware.
    ///
    /// Args:
    ///     log_params: If True, also log the action parameters (default: False).
    #[new]
    #[pyo3(signature = (log_params = false))]
    pub fn new(log_params: bool) -> Self {
        Self {
            inner: if log_params {
                LoggingMiddleware::with_params()
            } else {
                LoggingMiddleware::new()
            },
        }
    }

    /// Whether parameters are logged.
    #[getter]
    pub fn log_params(&self) -> bool {
        self.inner.log_params
    }

    fn __repr__(&self) -> String {
        format!("LoggingMiddleware(log_params={})", self.inner.log_params)
    }
}

// ── PyTimingMiddleware ───────────────────────────────────────────────────────

/// Timing middleware — measures per-action latency.
///
/// After each dispatch, elapsed time is available via :meth:`last_elapsed_ms`.
///
/// ```python
/// timing = TimingMiddleware()
/// pipeline.add_timing()
/// pipeline.dispatch("my_action", "{}")
/// print(timing.last_elapsed_ms("my_action"))  # milliseconds
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "TimingMiddleware")]
pub struct PyTimingMiddleware {
    pub(super) inner: Arc<TimingMiddleware>,
}

impl Default for PyTimingMiddleware {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyTimingMiddleware {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(TimingMiddleware::new()),
        }
    }

    /// Return the last recorded elapsed time for an action in milliseconds.
    ///
    /// Returns None if the action has not been dispatched yet.
    #[pyo3(signature = (action))]
    pub fn last_elapsed_ms(&self, action: &str) -> Option<u64> {
        self.inner
            .last_elapsed(action)
            .map(|d| d.as_millis() as u64)
    }

    fn __repr__(&self) -> String {
        "TimingMiddleware()".to_string()
    }
}

// ── PyAuditMiddleware ────────────────────────────────────────────────────────

/// Audit middleware — accumulates an in-memory log of all dispatched actions.
///
/// ```python
/// audit = AuditMiddleware()
/// pipeline.add_audit()
/// pipeline.dispatch("my_action", "{}")
/// for record in audit.records():
///     print(record["action"], record["success"])
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "AuditMiddleware")]
pub struct PyAuditMiddleware {
    pub(super) inner: Arc<AuditMiddleware>,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyAuditMiddleware {
    /// Create a new audit middleware.
    ///
    /// Args:
    ///     record_params: If True, include parameters in each audit record (default: True).
    #[new]
    #[pyo3(signature = (record_params = true))]
    pub fn new(record_params: bool) -> Self {
        let mut m = AuditMiddleware::new();
        m.record_params = record_params;
        Self { inner: Arc::new(m) }
    }

    /// Return all audit records as a list of dicts.
    ///
    /// Each dict has keys: ``action``, ``success``, ``error`` (str|None),
    /// ``output_preview`` (str|None), ``timestamp_ms`` (int).
    pub fn records<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let records = self.inner.records();
        records
            .iter()
            .map(|r| {
                let d = PyDict::new(py);
                d.set_item("action", &r.action)?;
                d.set_item("success", r.success)?;
                d.set_item(
                    "error",
                    r.error
                        .as_deref()
                        .map_or_else(|| py.None(), |e| e.into_pyobject(py).unwrap().into()),
                )?;
                d.set_item(
                    "output_preview",
                    r.output_preview
                        .as_deref()
                        .map_or_else(|| py.None(), |s| s.into_pyobject(py).unwrap().into()),
                )?;
                let ts_ms = r
                    .timestamp
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                d.set_item("timestamp_ms", ts_ms)?;
                Ok(d)
            })
            .collect()
    }

    /// Return audit records for a specific action name.
    pub fn records_for_action<'py>(
        &self,
        py: Python<'py>,
        action: &str,
    ) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let records = self.inner.records_for_action(action);
        records
            .iter()
            .map(|r| {
                let d = PyDict::new(py);
                d.set_item("action", &r.action)?;
                d.set_item("success", r.success)?;
                d.set_item(
                    "error",
                    r.error
                        .as_deref()
                        .map_or_else(|| py.None(), |e| e.into_pyobject(py).unwrap().into()),
                )?;
                d.set_item(
                    "output_preview",
                    r.output_preview
                        .as_deref()
                        .map_or_else(|| py.None(), |s| s.into_pyobject(py).unwrap().into()),
                )?;
                let ts_ms = r
                    .timestamp
                    .duration_since(std::time::UNIX_EPOCH)
                    .unwrap_or_default()
                    .as_millis() as u64;
                d.set_item("timestamp_ms", ts_ms)?;
                Ok(d)
            })
            .collect()
    }

    /// Return the number of recorded audit entries.
    pub fn record_count(&self) -> usize {
        self.inner.record_count()
    }

    /// Clear all audit records.
    pub fn clear(&self) {
        self.inner.clear();
    }

    fn __repr__(&self) -> String {
        format!("AuditMiddleware(count={})", self.inner.record_count())
    }
}

// ── PyRateLimitMiddleware ────────────────────────────────────────────────────

/// Rate limiting middleware — limits calls per action per time window.
///
/// Uses a fixed-window counter approach.  Once the limit is reached within
/// the window, dispatches return a ``RuntimeError``.
///
/// ```python
/// # Allow at most 5 calls per second
/// rate_limit = RateLimitMiddleware(max_calls=5, window_ms=1000)
/// pipeline.add_rate_limit(max_calls=5, window_ms=1000)
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "RateLimitMiddleware")]
pub struct PyRateLimitMiddleware {
    pub(super) inner: Arc<RateLimitMiddleware>,
    pub(super) max_calls: u64,
    pub(super) window_ms: u64,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyRateLimitMiddleware {
    /// Create a new rate limiter.
    ///
    /// Args:
    ///     max_calls:  Maximum allowed calls per action per ``window_ms``.
    ///     window_ms:  Window size in milliseconds.
    #[new]
    #[pyo3(signature = (max_calls, window_ms))]
    pub fn new(max_calls: u64, window_ms: u64) -> Self {
        Self {
            inner: Arc::new(RateLimitMiddleware::new(
                max_calls,
                Duration::from_millis(window_ms),
            )),
            max_calls,
            window_ms,
        }
    }

    /// Return the current call count for an action within the current window.
    #[pyo3(signature = (action))]
    pub fn call_count(&self, action: &str) -> u64 {
        self.inner.call_count(action)
    }

    /// Maximum allowed calls per window.
    #[getter]
    pub fn max_calls(&self) -> u64 {
        self.max_calls
    }

    /// Window size in milliseconds.
    #[getter]
    pub fn window_ms(&self) -> u64 {
        self.window_ms
    }

    fn __repr__(&self) -> String {
        format!(
            "RateLimitMiddleware(max_calls={}, window_ms={})",
            self.max_calls, self.window_ms
        )
    }
}
