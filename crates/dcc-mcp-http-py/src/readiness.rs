//! Python wrapper around [`dcc_mcp_skill_rest::StaticReadiness`] so
//! adapters (Maya, Blender, …) can share one probe instance between
//! the MCP `tools/call` surface and the REST `POST /v1/call` surface
//! (issue #714).
//!
//! Use [`PyMcpHttpServer::set_readiness_probe`](super::PyMcpHttpServer)
//! to install the probe on the server before starting it, then flip
//! the `dispatcher` / `dcc` bits from the DCC adapter's boot-complete
//! hook.

use std::sync::Arc;

use pyo3::prelude::*;

use dcc_mcp_skill_rest::{ReadinessProbe, StaticReadiness};

/// A three-state readiness probe (`process` / `dispatcher` / `dcc`) that
/// is shared between the MCP `tools/call` handler and the REST
/// `POST /v1/call` handler. Toggle the three bits as the DCC host
/// boots; `tools/call` refuses work with
/// `BACKEND_NOT_READY (-32002)` until the probe reports fully ready.
///
/// ``process`` defaults to ``True`` (the HTTP listener answers, so by
/// the time Python can construct this object the process is trivially
/// alive); ``dispatcher`` and ``dcc`` default to ``False``.
///
/// Example:
///
///     from dcc_mcp_core import ReadinessProbe, McpHttpServer, McpHttpConfig
///
///     probe = ReadinessProbe()
///     server = McpHttpServer(registry, McpHttpConfig(port=8765))
///     server.set_readiness_probe(probe)
///     server.start()
///
///     # ... later, once Maya's scripting engine is up:
///     probe.set_dispatcher_ready(True)
///     probe.set_dcc_ready(True)
#[pyclass(name = "ReadinessProbe", module = "dcc_mcp_core", skip_from_py_object)]
#[derive(Clone)]
pub struct PyReadinessProbe {
    pub(crate) inner: Arc<StaticReadiness>,
}

#[pymethods]
impl PyReadinessProbe {
    /// Construct a fresh probe. Starts in the not-ready state
    /// (``dispatcher=False``, ``dcc=False``) so `tools/call` refuses
    /// work until the adapter flips the bits.
    #[new]
    fn new() -> Self {
        Self {
            inner: Arc::new(StaticReadiness::new()),
        }
    }

    /// Start fully-ready. Convenient for standalone tests and servers
    /// with no embedded DCC to wait on.
    #[staticmethod]
    fn fully_ready() -> Self {
        Self {
            inner: Arc::new(StaticReadiness::fully_ready()),
        }
    }

    /// Toggle dispatcher readiness — call with ``True`` once the
    /// action dispatcher is wired.
    fn set_dispatcher_ready(&self, ready: bool) {
        self.inner.set_dispatcher_ready(ready);
    }

    /// Toggle DCC host readiness — call with ``True`` once the DCC's
    /// scripting engine / scene / etc. has finished booting.
    fn set_dcc_ready(&self, ready: bool) {
        self.inner.set_dcc_ready(ready);
    }

    /// Return ``True`` when all three bits are green.
    fn is_ready(&self) -> bool {
        self.inner.report().is_ready()
    }

    /// Return the current report as a ``dict`` with ``process`` /
    /// ``dispatcher`` / ``dcc`` keys.
    fn report(&self, py: Python<'_>) -> PyResult<Py<PyAny>> {
        use pyo3::types::PyDict;
        let report = self.inner.report();
        let d = PyDict::new(py);
        d.set_item("process", report.process)?;
        d.set_item("dispatcher", report.dispatcher)?;
        d.set_item("dcc", report.dcc)?;
        Ok(d.into_any().unbind())
    }

    fn __repr__(&self) -> String {
        let r = self.inner.report();
        format!(
            "ReadinessProbe(process={}, dispatcher={}, dcc={})",
            r.process, r.dispatcher, r.dcc
        )
    }
}

impl PyReadinessProbe {
    /// Return the shared [`Arc<dyn ReadinessProbe>`] for plumbing into
    /// the Rust server. Cheap clone of the inner `Arc`.
    pub(crate) fn as_dyn(&self) -> Arc<dyn ReadinessProbe> {
        self.inner.clone()
    }
}
