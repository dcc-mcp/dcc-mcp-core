//! `PyActionPipeline` — middleware-wrapped ActionDispatcher exposed to Python.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use pyo3::Py;
use pyo3::prelude::*;
use pyo3::types::PyDict;
use serde_json::Value;

#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use crate::dispatcher::{ActionDispatcher, DispatchError};
use crate::pipeline::{
    ActionPipeline, AuditMiddleware, LoggingMiddleware, RateLimitMiddleware, TimingMiddleware,
};

use super::helpers::{PyCallableHook, value_to_py};
use super::middleware::{PyAuditMiddleware, PyRateLimitMiddleware, PyTimingMiddleware};
use super::shared::{SharedAuditMiddleware, SharedRateLimitMiddleware, SharedTimingMiddleware};

/// Middleware-wrapped ActionDispatcher.
///
/// Allows attaching cross-cutting concerns (logging, timing, audit, rate
/// limiting) to action dispatch without modifying individual action handlers.
///
/// Middleware runs in registration order for ``before_dispatch``, and in
/// **reverse** order for ``after_dispatch`` (standard onion model).
///
/// ```python
/// import json
/// from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline
///
/// reg = ActionRegistry()
/// reg.register("ping", category="util")
///
/// dispatcher = ActionDispatcher(reg)
/// dispatcher.register_handler("ping", lambda params: "pong")
///
/// pipeline = ActionPipeline(dispatcher)
/// pipeline.add_logging()
/// pipeline.add_timing()
///
/// result = pipeline.dispatch("ping", "{}")
/// assert result["output"] == "pong"
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ToolPipeline")]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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

    // ── Middleware registration ──────────────────────────────────────────────

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

    // ── Dispatch ─────────────────────────────────────────────────────────────

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
            Err(err @ DispatchError::ActionDisabled { .. }) => Err(
                pyo3::exceptions::PyPermissionError::new_err(err.to_string()),
            ),
        }
    }

    // ── Introspection ────────────────────────────────────────────────────────

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
            "ToolPipeline(handlers={}, middleware={})",
            self.handler_map.len(),
            self.middleware_count
        )
    }
}
