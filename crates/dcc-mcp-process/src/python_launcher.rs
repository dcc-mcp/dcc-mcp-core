//! `PyDccLauncher` — async DCC process spawn / terminate / kill.

use std::sync::Arc;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::helpers::{map_process_err, runtime, status_to_str};
use crate::launcher::DccLauncher;
use crate::types::DccProcessConfig;

/// Async DCC process launcher (spawn / terminate / kill).
///
/// # Example (Python)
///
/// ```python
/// launcher = PyDccLauncher()
/// # info = launcher.launch("maya-2025", "/usr/autodesk/maya/bin/maya", [], 30000)
/// ```
#[pyclass(name = "PyDccLauncher")]
pub struct PyDccLauncher {
    inner: Arc<DccLauncher>,
}

#[pymethods]
impl PyDccLauncher {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Arc::new(DccLauncher::new()),
        }
    }

    /// Spawn a DCC process.
    ///
    /// Parameters
    /// ----------
    /// name : str
    ///     Logical name for this DCC instance.
    /// executable : str
    ///     Full path to the DCC executable.
    /// args : list[str], optional
    ///     Command-line arguments.
    /// launch_timeout_ms : int, optional
    ///     Milliseconds to wait for the process to start (default 30000).
    ///
    /// Returns a dict with ``pid``, ``name``, and ``status``.
    #[pyo3(signature = (name, executable, args=None, launch_timeout_ms=30000))]
    pub fn launch<'py>(
        &self,
        py: Python<'py>,
        name: &str,
        executable: &str,
        args: Option<Vec<String>>,
        launch_timeout_ms: u64,
    ) -> PyResult<Bound<'py, PyDict>> {
        let mut config = DccProcessConfig::new(name, executable);
        if let Some(a) = args {
            config.args = a;
        }
        config.launch_timeout_ms = launch_timeout_ms;

        let inner = Arc::clone(&self.inner);
        let rt = runtime()?;
        let info = rt
            .block_on(inner.launch(&config))
            .map_err(map_process_err)?;

        let d = PyDict::new(py);
        d.set_item("pid", info.pid)?;
        d.set_item("name", &info.name)?;
        d.set_item("status", status_to_str(info.status))?;
        Ok(d)
    }

    /// Gracefully terminate the named process.
    ///
    /// Parameters
    /// ----------
    /// name : str
    ///     The logical name used at launch time.
    /// timeout_ms : int, optional
    ///     Milliseconds to wait for the process to exit (default 5000).
    #[pyo3(signature = (name, timeout_ms=5000))]
    pub fn terminate(&self, name: &str, timeout_ms: u64) -> PyResult<()> {
        // Raise immediately if the process is not tracked.
        if self.inner.pid_of(name).is_none() {
            return Err(PyRuntimeError::new_err(format!(
                "process '{name}' is not running"
            )));
        }
        let inner = Arc::clone(&self.inner);
        let name = name.to_string();
        let rt = runtime()?;
        rt.block_on(inner.terminate(&name, timeout_ms))
            .map_err(map_process_err)
    }

    /// Forcefully kill the named process.
    pub fn kill(&self, name: &str) -> PyResult<()> {
        // Raise immediately if the process is not tracked.
        if self.inner.pid_of(name).is_none() {
            return Err(PyRuntimeError::new_err(format!(
                "process '{name}' is not running"
            )));
        }
        let inner = Arc::clone(&self.inner);
        let name = name.to_string();
        let rt = runtime()?;
        rt.block_on(inner.kill(&name)).map_err(map_process_err)
    }

    /// Return the PID of the named running child, or `None`.
    pub fn pid_of(&self, name: &str) -> Option<u32> {
        self.inner.pid_of(name)
    }

    /// Return the number of currently tracked live children.
    pub fn running_count(&self) -> usize {
        self.inner.running_count()
    }

    /// Return the restart count for the given name.
    pub fn restart_count(&self, name: &str) -> u32 {
        self.inner.restart_count(name)
    }

    pub fn __repr__(&self) -> String {
        format!("PyDccLauncher(running={})", self.inner.running_count())
    }
}

impl Default for PyDccLauncher {
    fn default() -> Self {
        Self::new()
    }
}
