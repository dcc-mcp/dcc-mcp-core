//! Internal helpers shared across every `Py*` wrapper in [`crate::python`].
//!
//! Keeps the shared Tokio runtime, the `ProcessError → PyErr` adaptor,
//! and the `ProcessStatus`-as-string serialiser in one place so the
//! per-class bindings stay focused on their own surface.

use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use tokio::runtime::Runtime;

use crate::error::ProcessError;
use crate::types::ProcessStatus;

pub(super) fn runtime() -> PyResult<Arc<Runtime>> {
    static RT: std::sync::OnceLock<Arc<Runtime>> = std::sync::OnceLock::new();
    RT.get_or_init(|| {
        Arc::new(
            tokio::runtime::Builder::new_multi_thread()
                .worker_threads(2)
                .enable_all()
                .build()
                .expect("failed to build process tokio runtime"),
        )
    });
    Ok(Arc::clone(
        RT.get().expect("OnceLock initialized by get_or_init above"),
    ))
}

pub(super) fn map_process_err(e: ProcessError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Convert `ProcessStatus` to its string representation.
pub(super) fn status_to_str(s: ProcessStatus) -> &'static str {
    match s {
        ProcessStatus::Running => "running",
        ProcessStatus::Starting => "starting",
        ProcessStatus::Stopped => "stopped",
        ProcessStatus::Crashed => "crashed",
        ProcessStatus::Unresponsive => "unresponsive",
        ProcessStatus::Restarting => "restarting",
    }
}
