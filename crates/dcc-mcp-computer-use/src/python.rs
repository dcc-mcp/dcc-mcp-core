//! PyO3 bindings for the native computer-use session.

use pyo3::exceptions::PyValueError;
use pyo3::prelude::*;
use pyo3::types::PyBytes;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};
use serde_json::json;

use crate::{ComputerUseAction, ComputerUseSession, ComputerUseTargetScope};

fn runtime_scope_value<T>(name: &str) -> PyResult<Option<T>>
where
    T: std::str::FromStr,
{
    let Some(raw) = std::env::var_os(name) else {
        return Ok(None);
    };
    let raw = raw.to_string_lossy();
    let raw = raw.trim();
    if raw.is_empty() {
        return Ok(None);
    }
    raw.parse::<T>().map(Some).map_err(|_| {
        PyValueError::new_err(format!(
            "{name} must contain a positive decimal native identifier"
        ))
    })
}

fn runtime_target_scope() -> PyResult<ComputerUseTargetScope> {
    let process_id = runtime_scope_value::<u32>("DCC_MCP_UI_CONTROL_UIA_PROCESS_ID")?;
    let window_handle = runtime_scope_value::<u64>("DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE")?;
    ComputerUseTargetScope::new(process_id, window_handle).map_err(|error| {
        PyValueError::new_err(format!(
            "{}; bind the adapter at process launch with DCC_MCP_UI_CONTROL_UIA_PROCESS_ID or DCC_MCP_UI_CONTROL_UIA_WINDOW_HANDLE",
            error.message
        ))
    })
}

/// Native, scoped DCC MCP Computer Use session.
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
#[pyclass(name = "ComputerUseSession")]
pub struct PyComputerUseSession {
    inner: ComputerUseSession,
}

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
#[pymethods]
impl PyComputerUseSession {
    /// Create a session for exactly one application window.
    #[new]
    #[pyo3(signature = (*, process_id=None, window_handle=None, window_title=None, app_name=None))]
    fn new(
        process_id: Option<u32>,
        window_handle: Option<u64>,
        window_title: Option<String>,
        app_name: Option<String>,
    ) -> PyResult<Self> {
        let trusted_scope = runtime_target_scope()?;
        let inner = ComputerUseSession::new(
            trusted_scope,
            process_id,
            window_handle,
            window_title,
            app_name,
            None, // session_id not yet exposed through Python bindings
        )
        .map_err(|error| PyValueError::new_err(error.message))?;
        Ok(Self { inner })
    }

    /// Return whether Esc stopped Computer Use in this Windows logon session.
    #[staticmethod]
    fn process_user_interrupted() -> bool {
        crate::platform::user_interrupted()
    }

    /// Return whether this process can currently observe the interactive Windows desktop.
    #[staticmethod]
    fn desktop_interactive() -> bool {
        crate::platform::desktop_interactive()
    }

    /// Start the visible banner and reserve Esc for the stop action.
    fn start(&self) -> String {
        let result = Python::attach(|py| py.detach(|| self.inner.start()));
        match result {
            Ok(value) => value.to_string(),
            Err(error) => error.to_json().to_string(),
        }
    }

    /// Return `(metadata_json, png_bytes_or_none)` for the scoped window.
    fn screenshot(&self, py: Python<'_>) -> (String, Option<Py<PyBytes>>) {
        let result = py.detach(|| self.inner.screenshot());
        match result {
            Ok(screenshot) => {
                let metadata = json!({
                    "success": true,
                    "observation": screenshot.observation,
                    "mime_type": "image/png",
                });
                (
                    metadata.to_string(),
                    Some(PyBytes::new(py, &screenshot.data).unbind()),
                )
            }
            Err(error) => (error.to_json().to_string(), None),
        }
    }

    /// Perform one JSON-encoded native action.
    fn act(&self, request_json: &str) -> String {
        let request: ComputerUseAction = match serde_json::from_str(request_json) {
            Ok(request) => request,
            Err(error) => {
                return json!({
                    "success": false,
                    "error": "invalid_action",
                    "message": format!("invalid computer-use action JSON: {error}"),
                })
                .to_string();
            }
        };
        let result = Python::attach(|py| py.detach(|| self.inner.perform(&request)));
        match result {
            Ok(value) => value.to_string(),
            Err(error) => error.to_json().to_string(),
        }
    }

    /// Request an immediate stop without waiting for the active action.
    fn request_stop(&self) {
        self.inner.request_stop();
    }

    /// Stop the session and remove the banner.
    fn stop(&self) -> String {
        Python::attach(|py| py.detach(|| self.inner.stop())).to_string()
    }

    /// Clear the Windows-logon-session stop latch after explicit user approval.
    fn resume_after_user_approval(&self) -> String {
        self.inner.resume_after_user_approval().to_string()
    }

    /// Return the current session state as JSON.
    fn status(&self) -> String {
        self.inner.status().to_string()
    }
}

/// Register computer-use bindings on `dcc_mcp_core._core`.
pub fn register_classes(module: &Bound<'_, PyModule>) -> PyResult<()> {
    module.add_class::<PyComputerUseSession>()
}
