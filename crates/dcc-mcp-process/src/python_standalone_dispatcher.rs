//! `PyStandaloneDispatcher` — reference host dispatcher for non-DCC environments.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::helpers::{map_process_err, runtime};
use crate::dispatcher::{HostDispatcher, JobRequest, StandaloneDispatcher, ThreadAffinity};
use crate::error::ProcessError;

/// Reference host dispatcher implementation for non-DCC environments.
///
/// Supports only ``ThreadAffinity.Any`` and executes submitted jobs on a Tokio
/// worker task. Useful for tests and standalone CLI integrations.
#[pyclass(name = "PyStandaloneDispatcher")]
pub struct PyStandaloneDispatcher {
    inner: StandaloneDispatcher,
}

#[pymethods]
impl PyStandaloneDispatcher {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: StandaloneDispatcher::new(),
        }
    }

    /// Submit a lightweight job and block for completion.
    ///
    /// Parameters
    /// ----------
    /// action_name : str
    ///     Logical action identifier used as request_id.
    /// payload : str, optional
    ///     Opaque payload text, returned in the output on success.
    /// affinity : str, optional
    ///     ``"any"`` (default) or ``"main"``.
    ///     Standalone dispatcher only supports ``"any"``; ``"main"`` will
    ///     return an error outcome.
    #[pyo3(signature = (action_name, payload=None, affinity="any"))]
    pub fn submit<'py>(
        &self,
        py: Python<'py>,
        action_name: &str,
        payload: Option<String>,
        affinity: &str,
    ) -> PyResult<Bound<'py, PyDict>> {
        let affinity = match affinity.to_ascii_lowercase() {
            s if s == "any" => ThreadAffinity::Any,
            s if s == "main" => ThreadAffinity::Main,
            _ => {
                return Err(PyValueError::new_err(
                    "affinity must be one of: any, main (standalone only supports 'any')",
                ));
            }
        };

        let payload_val = payload.unwrap_or_default();
        let req = JobRequest::new(
            action_name,
            affinity,
            Box::new(move || Ok(serde_json::Value::String(payload_val))),
        );
        let rt = runtime()?;
        let outcome = rt
            .block_on(async {
                let rx = self.inner.submit(req);
                rx.await.map_err(|e| ProcessError::internal(e.to_string()))
            })
            .map_err(map_process_err)?;

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
}

impl Default for PyStandaloneDispatcher {
    fn default() -> Self {
        Self::new()
    }
}
