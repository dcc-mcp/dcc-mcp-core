//! PyO3 bindings for the ActionPipeline and its built-in middleware.
//!
//! Exposed Python classes:
//!
//! - [`PyLoggingMiddleware`]    — emits tracing log lines before/after each action
//! - [`PyTimingMiddleware`]     — measures per-action latency (queryable from Python)
//! - [`PyAuditMiddleware`]      — accumulates an in-memory audit log (queryable from Python)
//! - [`PyRateLimitMiddleware`]  — fixed-window rate limiter per action name
//! - [`PyActionPipeline`]       — middleware-wrapped ActionDispatcher
//!
//! ## Design
//!
//! `PyAuditMiddleware` and `PyRateLimitMiddleware` expose mutable state to Python;
//! they are stored behind `Arc` so the pipeline and the Python handle share the
//! same instance.
//!
//! `PyActionPipeline` reuses the `PyActionDispatcher` from `crate::python` for
//! handler registration, then delegates through the Rust `ActionPipeline` for
//! middleware processing.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use pyo3::Py;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

use crate::dispatcher::{ActionDispatcher, DispatchError, DispatchResult};
use crate::pipeline::{
    ActionMiddleware, ActionPipeline, AuditMiddleware, LoggingMiddleware, MiddlewareContext,
    RateLimitMiddleware, TimingMiddleware,
};

// ── Helper: convert serde_json::Value to Python ──────────────────────────────

fn value_to_py(py: Python<'_>, value: &Value) -> PyResult<Py<PyAny>> {
    match value {
        Value::Null => Ok(py.None()),
        Value::Bool(b) => Ok((*b).into_pyobject(py)?.to_owned().into()),
        Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                Ok(i.into_pyobject(py)?.into())
            } else {
                Ok(n.as_f64().unwrap_or(f64::NAN).into_pyobject(py)?.into())
            }
        }
        Value::String(s) => Ok(s.as_str().into_pyobject(py)?.into()),
        Value::Array(arr) => {
            let list = pyo3::types::PyList::empty(py);
            for v in arr {
                list.append(value_to_py(py, v)?)?;
            }
            Ok(list.into())
        }
        Value::Object(map) => {
            let dict = PyDict::new(py);
            for (k, v) in map {
                dict.set_item(k, value_to_py(py, v)?)?;
            }
            Ok(dict.into())
        }
    }
}

// ── PyCallableMiddleware ──────────────────────────────────────────────────────

/// Python callable pair that acts as middleware.
///
/// Unlike the Rust `ActionMiddleware` trait, these are called directly from
/// `PyActionPipeline::dispatch()` because they need the GIL, which is already
/// held inside `#[pymethods]`.
struct PyCallableHook {
    before_fn: Option<Py<PyAny>>,
    after_fn: Option<Py<PyAny>>,
}

// ── PyLoggingMiddleware ───────────────────────────────────────────────────────

/// Logging middleware — emits tracing log lines before/after each action.
///
/// Example::
///
///     from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline, LoggingMiddleware
///
///     reg = ActionRegistry()
///     dispatcher = ActionDispatcher(reg)
///     pipeline = ActionPipeline(dispatcher)
///     pipeline.add_logging(log_params=True)
///
#[pyclass(name = "LoggingMiddleware")]
pub struct PyLoggingMiddleware {
    inner: LoggingMiddleware,
}

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

// ── PyTimingMiddleware ────────────────────────────────────────────────────────

/// Timing middleware — measures per-action latency.
///
/// After each dispatch, elapsed time is available via :meth:`last_elapsed_ms`.
///
/// Example::
///
///     timing = TimingMiddleware()
///     pipeline.add_timing()
///     pipeline.dispatch("my_action", "{}")
///     print(timing.last_elapsed_ms("my_action"))  # milliseconds
///
#[pyclass(name = "TimingMiddleware")]
pub struct PyTimingMiddleware {
    inner: Arc<TimingMiddleware>,
}

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

// ── PyAuditMiddleware ─────────────────────────────────────────────────────────

/// Audit middleware — accumulates an in-memory log of all dispatched actions.
///
/// Example::
///
///     audit = AuditMiddleware()
///     pipeline.add_audit()
///     pipeline.dispatch("my_action", "{}")
///     for record in audit.records():
///         print(record["action"], record["success"])
///
#[pyclass(name = "AuditMiddleware")]
pub struct PyAuditMiddleware {
    inner: Arc<AuditMiddleware>,
}

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

// ── PyRateLimitMiddleware ─────────────────────────────────────────────────────

/// Rate limiting middleware — limits calls per action per time window.
///
/// Uses a fixed-window counter approach.  Once the limit is reached within
/// the window, dispatches return a ``RuntimeError``.
///
/// Example::
///
///     # Allow at most 5 calls per second
///     rate_limit = RateLimitMiddleware(max_calls=5, window_ms=1000)
///     pipeline.add_rate_limit(max_calls=5, window_ms=1000)
///
#[pyclass(name = "RateLimitMiddleware")]
pub struct PyRateLimitMiddleware {
    inner: Arc<RateLimitMiddleware>,
    max_calls: u64,
    window_ms: u64,
}

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

// ── PyActionPipeline ──────────────────────────────────────────────────────────

/// Middleware-wrapped ActionDispatcher.
///
/// Allows attaching cross-cutting concerns (logging, timing, audit, rate
/// limiting) to action dispatch without modifying individual action handlers.
///
/// Middleware runs in registration order for ``before_dispatch``, and in
/// **reverse** order for ``after_dispatch`` (standard onion model).
///
/// Example::
///
///     import json
///     from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline
///
///     reg = ActionRegistry()
///     reg.register("ping", category="util")
///
///     dispatcher = ActionDispatcher(reg)
///     dispatcher.register_handler("ping", lambda params: "pong")
///
///     pipeline = ActionPipeline(dispatcher)
///     pipeline.add_logging()
///     pipeline.add_timing()
///
///     result = pipeline.dispatch("ping", "{}")
///     assert result["output"] == "pong"
///
#[pyclass(name = "ActionPipeline")]
pub struct PyActionPipeline {
    /// Rust-level pipeline wrapping an `ActionDispatcher`.
    inner: ActionPipeline,
    /// Python-level handler map (mirrors `PyActionDispatcher.handler_map`).
    handler_map: HashMap<String, Py<PyAny>>,
    /// Python callable hooks (before/after) added via `add_callable()`.
    callable_hooks: Vec<PyCallableHook>,
    /// Number of middleware registered (for repr).
    middleware_count: usize,
    /// Names of registered middleware.
    middleware_names: Vec<String>,
}

#[pymethods]
impl PyActionPipeline {
    /// Create a pipeline wrapping the given ``ActionDispatcher``.
    ///
    /// The dispatcher's handlers are copied; changes after construction are
    /// **not** reflected.  Register handlers before building the pipeline.
    ///
    /// Args:
    ///     dispatcher: An :class:`ActionDispatcher` with handlers already registered.
    #[new]
    pub fn new(dispatcher: &crate::python::PyActionDispatcher) -> Self {
        // Extract the registry and handler map from the existing PyActionDispatcher
        let registry = dispatcher.registry();
        let rust_dispatcher = ActionDispatcher::new(registry);
        // Copy handlers from PyActionDispatcher
        let handler_map = dispatcher.handler_map_clone();
        for name in handler_map.keys() {
            rust_dispatcher.register_handler(name, |_| Ok(Value::Null));
        }
        Self {
            inner: ActionPipeline::new(rust_dispatcher),
            handler_map,
            callable_hooks: Vec::new(),
            middleware_count: 0,
            middleware_names: Vec::new(),
        }
    }

    // ── Middleware registration ───────────────────────────────────────────────

    /// Add a :class:`LoggingMiddleware` to the pipeline.
    ///
    /// Args:
    ///     log_params: If ``True``, log action parameters (default: ``False``).
    #[pyo3(signature = (log_params = false))]
    pub fn add_logging(&mut self, log_params: bool) {
        let m = if log_params {
            LoggingMiddleware::with_params()
        } else {
            LoggingMiddleware::new()
        };
        self.inner.add_middleware(m);
        self.middleware_count += 1;
        self.middleware_names.push("logging".to_string());
    }

    /// Add a :class:`TimingMiddleware` to the pipeline.
    ///
    /// Returns the middleware instance so the caller can query timings.
    pub fn add_timing(&mut self) -> PyTimingMiddleware {
        let timing = Arc::new(TimingMiddleware::new());
        let timing_clone = Arc::clone(&timing);
        // Wrap in a newtype that implements ActionMiddleware but defers to the Arc
        self.inner
            .add_middleware(SharedTimingMiddleware(timing_clone));
        self.middleware_count += 1;
        self.middleware_names.push("timing".to_string());
        PyTimingMiddleware { inner: timing }
    }

    /// Add an :class:`AuditMiddleware` to the pipeline.
    ///
    /// Returns the middleware instance so the caller can query records.
    ///
    /// Args:
    ///     record_params: If ``True``, include parameters in audit records (default: ``True``).
    #[pyo3(signature = (record_params = true))]
    pub fn add_audit(&mut self, record_params: bool) -> PyAuditMiddleware {
        let mut audit = AuditMiddleware::new();
        audit.record_params = record_params;
        let audit_arc = Arc::new(audit);
        let audit_clone = Arc::clone(&audit_arc);
        self.inner
            .add_middleware(SharedAuditMiddleware(audit_clone));
        self.middleware_count += 1;
        self.middleware_names.push("audit".to_string());
        PyAuditMiddleware { inner: audit_arc }
    }

    /// Add a :class:`RateLimitMiddleware` to the pipeline.
    ///
    /// Returns the middleware instance so the caller can query counters.
    ///
    /// Args:
    ///     max_calls:  Maximum allowed calls per action per ``window_ms``.
    ///     window_ms:  Window size in milliseconds.
    #[pyo3(signature = (max_calls, window_ms))]
    pub fn add_rate_limit(&mut self, max_calls: u64, window_ms: u64) -> PyRateLimitMiddleware {
        let rl = Arc::new(RateLimitMiddleware::new(
            max_calls,
            Duration::from_millis(window_ms),
        ));
        let rl_clone = Arc::clone(&rl);
        self.inner
            .add_middleware(SharedRateLimitMiddleware(rl_clone));
        self.middleware_count += 1;
        self.middleware_names.push("rate_limit".to_string());
        PyRateLimitMiddleware {
            inner: rl,
            max_calls,
            window_ms,
        }
    }

    /// Add a custom Python callable middleware.
    ///
    /// Args:
    ///     before_fn: Optional callable ``(action_name: str) -> None`` called before dispatch.
    ///     after_fn:  Optional callable ``(action_name: str, success: bool) -> None`` called after.
    #[pyo3(signature = (before_fn = None, after_fn = None))]
    pub fn add_callable(
        &mut self,
        py: Python<'_>,
        before_fn: Option<Py<PyAny>>,
        after_fn: Option<Py<PyAny>>,
    ) -> PyResult<()> {
        if let Some(ref f) = before_fn {
            if !f.bind(py).is_callable() {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "before_fn must be callable",
                ));
            }
        }
        if let Some(ref f) = after_fn {
            if !f.bind(py).is_callable() {
                return Err(pyo3::exceptions::PyTypeError::new_err(
                    "after_fn must be callable",
                ));
            }
        }
        self.callable_hooks.push(PyCallableHook {
            before_fn,
            after_fn,
        });
        self.middleware_count += 1;
        self.middleware_names.push("python_callable".to_string());
        Ok(())
    }

    // ── Dispatch ──────────────────────────────────────────────────────────────

    /// Dispatch an action through the middleware pipeline.
    ///
    /// Runs all middleware hooks, then calls the registered Python handler.
    ///
    /// Args:
    ///     action_name: Name of the registered action.
    ///     params_json: JSON-encoded parameters (default: ``"null"``).
    ///
    /// Returns:
    ///     A dict with keys ``"action"`` (str), ``"output"`` (Any), ``"validation_skipped"`` (bool).
    ///
    /// Raises:
    ///     KeyError:     No handler registered for ``action_name``.
    ///     ValueError:   Invalid JSON or schema validation failure.
    ///     RuntimeError: Handler error or rate-limit exceeded.
    #[pyo3(signature = (action_name, params_json = "null"))]
    pub fn dispatch<'py>(
        &self,
        py: Python<'py>,
        action_name: &str,
        params_json: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let params: Value = serde_json::from_str(params_json)
            .map_err(|e| pyo3::exceptions::PyValueError::new_err(format!("invalid JSON: {e}")))?;

        // Run Python callable before_fn hooks
        for hook in &self.callable_hooks {
            if let Some(f) = &hook.before_fn {
                f.call1(py, (action_name,)).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("before_fn error: {e}"))
                })?;
            }
        }

        // Run through Rust middleware pipeline (stubs return null)
        let result = self.inner.dispatch(action_name, params.clone());

        // Determine success for Python after_fn hooks
        let success = result.is_ok();

        // Run Python callable after_fn hooks (in reverse order)
        for hook in self.callable_hooks.iter().rev() {
            if let Some(f) = &hook.after_fn {
                let _ = f.call1(py, (action_name, success));
            }
        }

        match result {
            Ok(dispatch_result) => {
                // Call the real Python handler
                let handler = self.handler_map.get(action_name).ok_or_else(|| {
                    pyo3::exceptions::PyKeyError::new_err(format!("no handler for '{action_name}'"))
                })?;
                let py_params = value_to_py(py, &params)?;
                let raw = handler.call1(py, (py_params,)).map_err(|e| {
                    pyo3::exceptions::PyRuntimeError::new_err(format!("handler error: {e}"))
                })?;
                let d = PyDict::new(py);
                d.set_item("action", action_name)?;
                d.set_item("output", raw)?;
                d.set_item("validation_skipped", dispatch_result.validation_skipped)?;
                Ok(d)
            }
            Err(DispatchError::HandlerNotFound(_)) => Err(pyo3::exceptions::PyKeyError::new_err(
                format!("no handler for '{action_name}'"),
            )),
            Err(DispatchError::ValidationFailed(msg)) => Err(
                pyo3::exceptions::PyValueError::new_err(format!("validation failed: {msg}")),
            ),
            Err(DispatchError::MetadataNotFound(name)) => {
                Err(pyo3::exceptions::PyKeyError::new_err(format!(
                    "action metadata not found: '{name}'"
                )))
            }
            Err(DispatchError::HandlerError(msg)) => {
                Err(pyo3::exceptions::PyRuntimeError::new_err(msg))
            }
        }
    }

    // ── Introspection ─────────────────────────────────────────────────────────

    /// Register a Python callable as a handler for ``action_name``.
    ///
    /// This mirrors ``ActionDispatcher.register_handler`` but operates on the
    /// pipeline's internal dispatcher.
    #[pyo3(signature = (action_name, handler))]
    pub fn register_handler(
        &mut self,
        py: Python<'_>,
        action_name: &str,
        handler: Py<PyAny>,
    ) -> PyResult<()> {
        if !handler.bind(py).is_callable() {
            return Err(pyo3::exceptions::PyTypeError::new_err(
                "handler must be callable",
            ));
        }
        self.handler_map.insert(action_name.to_string(), handler);
        self.inner
            .dispatcher()
            .register_handler(action_name, |_| Ok(Value::Null));
        Ok(())
    }

    /// Number of middleware currently registered.
    pub fn middleware_count(&self) -> usize {
        self.middleware_count
    }

    /// Names of registered middleware in registration order.
    pub fn middleware_names(&self) -> Vec<String> {
        self.middleware_names.clone()
    }

    /// Number of handlers registered.
    pub fn handler_count(&self) -> usize {
        self.handler_map.len()
    }

    fn __repr__(&self) -> String {
        format!(
            "ActionPipeline(handlers={}, middleware={})",
            self.handler_map.len(),
            self.middleware_count
        )
    }
}

// ── Shared Arc wrappers implementing ActionMiddleware ────────────────────────

/// Newtype wrapper so `Arc<TimingMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedTimingMiddleware(pub(crate) Arc<TimingMiddleware>);

impl ActionMiddleware for SharedTimingMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "timing"
    }
}

/// Newtype wrapper so `Arc<AuditMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedAuditMiddleware(pub(crate) Arc<AuditMiddleware>);

impl ActionMiddleware for SharedAuditMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "audit"
    }
}

/// Newtype wrapper so `Arc<RateLimitMiddleware>` can be passed to `add_middleware`.
pub(crate) struct SharedRateLimitMiddleware(pub(crate) Arc<RateLimitMiddleware>);

impl ActionMiddleware for SharedRateLimitMiddleware {
    fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
        self.0.before_dispatch(ctx)
    }

    fn after_dispatch(
        &self,
        ctx: &MiddlewareContext,
        result: Result<&DispatchResult, &DispatchError>,
    ) {
        self.0.after_dispatch(ctx, result);
    }

    fn name(&self) -> &'static str {
        "rate_limit"
    }
}

// ── Registration ─────────────────────────────────────────────────────────────

/// Register all pipeline Python classes on the given module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyLoggingMiddleware>()?;
    m.add_class::<PyTimingMiddleware>()?;
    m.add_class::<PyAuditMiddleware>()?;
    m.add_class::<PyRateLimitMiddleware>()?;
    m.add_class::<PyActionPipeline>()?;
    Ok(())
}
