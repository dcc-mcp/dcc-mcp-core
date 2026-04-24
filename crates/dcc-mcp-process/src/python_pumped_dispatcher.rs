//! `PyPumpedDispatcher` — thread-affinity aware dispatcher with cooperative
//! main-thread pump, plus the `parse_affinity` / `outcome_to_dict`
//! helpers shared only with this binding.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::helpers::{map_process_err, runtime};
use crate::dispatcher::{ActionOutcome, HostDispatcher, JobRequest, ThreadAffinity};
use crate::error::ProcessError;
use crate::pump::PumpedDispatcher;

/// Thread-affinity aware dispatcher with cooperative main-thread pump.
///
/// Combines an ``ipckit::MainThreadPump`` for ``"main"``/``"named"`` affinity
/// jobs with Tokio worker execution for ``"any"`` affinity jobs.
///
/// Call :meth:`pump` from the DCC host's idle callback (e.g. Maya
/// ``scriptJob(idleEvent=...)``) to drain pending main-thread work items.
///
/// # Example (Python)
///
/// ```python
/// from dcc_mcp_core import PumpedDispatcher
///
/// dispatcher = PumpedDispatcher(budget_ms=8)
/// result = dispatcher.submit("update-ui", payload="refresh", affinity="main")
/// # In idle callback:
/// stats = dispatcher.pump()
/// print(f"processed={stats['processed']}, remaining={stats['remaining']}")
/// ```
#[pyclass(name = "PyPumpedDispatcher")]
pub struct PyPumpedDispatcher {
    inner: PumpedDispatcher,
}

#[pymethods]
impl PyPumpedDispatcher {
    /// Create a new pumped dispatcher.
    ///
    /// Parameters
    /// ----------
    /// budget_ms : int, optional
    ///     Wall-clock budget per ``pump()`` call in milliseconds (default 8).
    #[new]
    #[pyo3(signature = (budget_ms=8))]
    pub fn new(budget_ms: u64) -> Self {
        Self {
            inner: PumpedDispatcher::new(std::time::Duration::from_millis(budget_ms)),
        }
    }

    /// Drain pending main-thread work items.
    ///
    /// Call from the DCC host's idle/update callback. Processes as many
    /// main-thread jobs as possible within the configured budget.
    ///
    /// Returns a dict with ``processed`` and ``remaining`` counts.
    pub fn pump<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let stats = self.inner.pump();
        let d = PyDict::new(py);
        d.set_item("processed", stats.processed)?;
        d.set_item("remaining", stats.remaining)?;
        Ok(d)
    }

    /// Drain with an explicit budget override for this call only.
    ///
    /// Parameters
    /// ----------
    /// budget_ms : int
    ///     Budget in milliseconds for this pump call.
    pub fn pump_with_budget<'py>(
        &self,
        py: Python<'py>,
        budget_ms: u64,
    ) -> PyResult<Bound<'py, PyDict>> {
        let stats = self
            .inner
            .pump_with_budget(std::time::Duration::from_millis(budget_ms));
        let d = PyDict::new(py);
        d.set_item("processed", stats.processed)?;
        d.set_item("remaining", stats.remaining)?;
        Ok(d)
    }

    /// Submit a job and block for completion.
    ///
    /// Parameters
    /// ----------
    /// action_name : str
    ///     Logical action identifier used as request_id.
    /// payload : str, optional
    ///     Opaque payload text, returned in the output on success.
    /// affinity : str, optional
    ///     ``"any"`` (default), ``"main"``, or ``"named:ThreadName"``.
    ///     Main/named jobs are drained by ``pump()``; any jobs run on Tokio.
    #[pyo3(signature = (action_name, payload=None, affinity="any"))]
    pub fn submit<'py>(
        &self,
        py: Python<'py>,
        action_name: &str,
        payload: Option<String>,
        affinity: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let aff = parse_affinity(affinity)?;

        let payload_val = payload.unwrap_or_default();
        let req = JobRequest::new(
            action_name,
            aff,
            Box::new(move || Ok(serde_json::Value::String(payload_val))),
        );

        let rt = runtime()?;
        let inner = self.inner.clone();

        let outcome = rt
            .block_on(async {
                let rx = inner.submit(req);
                rx.await.map_err(|e| ProcessError::internal(e.to_string()))
            })
            .map_err(map_process_err)?;

        outcome_to_dict(py, outcome)
    }

    /// Number of main-thread items currently waiting.
    pub fn pending(&self) -> usize {
        self.inner.pending()
    }

    /// Total items ever dispatched to the main-thread pump.
    #[getter]
    pub fn total_dispatched(&self) -> u64 {
        self.inner.total_dispatched()
    }

    /// Total items ever processed by the main-thread pump.
    #[getter]
    pub fn total_processed(&self) -> u64 {
        self.inner.total_processed()
    }

    /// Configured budget in milliseconds.
    #[getter]
    pub fn budget_ms(&self) -> u64 {
        self.inner.budget().as_millis() as u64
    }

    /// Return supported affinity values.
    pub fn supported(&self) -> Vec<String> {
        self.inner
            .supported()
            .iter()
            .map(std::string::ToString::to_string)
            .collect()
    }

    /// Return dispatcher capability flags as a dict.
    pub fn capabilities<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let caps = self.inner.capabilities();
        let d = PyDict::new(py);
        d.set_item("supports_main_thread", caps.supports_main_thread)?;
        d.set_item("supports_named_threads", caps.supports_named_threads)?;
        d.set_item("supports_any_thread", caps.supports_any_thread)?;
        d.set_item("supports_time_slicing", caps.supports_time_slicing)?;
        Ok(d)
    }

    pub fn __repr__(&self) -> String {
        format!(
            "PumpedDispatcher(pending={}, budget_ms={})",
            self.inner.pending(),
            self.inner.budget().as_millis()
        )
    }
}

fn parse_affinity(s: &str) -> PyResult<ThreadAffinity> {
    let lower = s.to_ascii_lowercase();
    if lower == "any" {
        Ok(ThreadAffinity::Any)
    } else if lower == "main" {
        Ok(ThreadAffinity::Main)
    } else if let Some(name) = lower.strip_prefix("named:") {
        // Leak the string to get &'static str — acceptable for thread names
        // which are typically a small fixed set.
        let static_str: &'static str = Box::leak(name.to_string().into_boxed_str());
        Ok(ThreadAffinity::Named(static_str))
    } else {
        Err(PyValueError::new_err(format!(
            "affinity must be one of: 'any', 'main', 'named:ThreadName'; got '{s}'"
        )))
    }
}

fn outcome_to_dict<'py>(py: Python<'py>, outcome: ActionOutcome) -> PyResult<Bound<'py, PyDict>> {
    let d = PyDict::new(py);
    d.set_item("request_id", outcome.request_id)?;
    d.set_item("affinity", outcome.affinity.to_string())?;
    d.set_item("success", outcome.success)?;
    match outcome.output {
        Some(v) => d.set_item("output", v.to_string())?,
        None => d.set_item("output", py.None())?,
    }
    match outcome.error {
        Some(e) => d.set_item("error", e)?,
        None => d.set_item("error", py.None())?,
    }
    Ok(d)
}
