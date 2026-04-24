//! `PyProcessMonitor` — cross-platform DCC process monitor binding.

use std::sync::Mutex;

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;

use super::helpers::status_to_str;
use crate::monitor::ProcessMonitor;

/// Cross-platform DCC process monitor.
///
/// Wraps `ProcessMonitor` — tracks live resource snapshots via `sysinfo`.
///
/// # Example (Python)
///
/// ```python
/// mon = PyProcessMonitor()
/// import os
/// mon.track(os.getpid(), "self")
/// mon.refresh()
/// info = mon.query(os.getpid())
/// print(info["status"])  # "running"
/// ```
#[pyclass(name = "PyProcessMonitor")]
pub struct PyProcessMonitor {
    inner: Mutex<ProcessMonitor>,
}

#[pymethods]
impl PyProcessMonitor {
    #[new]
    pub fn new() -> Self {
        Self {
            inner: Mutex::new(ProcessMonitor::new()),
        }
    }

    /// Register a PID to monitor.
    pub fn track(&self, pid: u32, name: &str) -> PyResult<()> {
        self.inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .track(pid, name);
        Ok(())
    }

    /// Stop monitoring a PID.
    pub fn untrack(&self, pid: u32) -> PyResult<()> {
        self.inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .untrack(pid);
        Ok(())
    }

    /// Refresh underlying system data.  Must be called before querying.
    pub fn refresh(&self) -> PyResult<()> {
        self.inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .refresh();
        Ok(())
    }

    /// Return a dict snapshot for `pid`, or `None` if not found.
    pub fn query<'py>(&self, py: Python<'py>, pid: u32) -> PyResult<Option<Bound<'py, PyDict>>> {
        let info = self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .query(pid);

        match info {
            None => Ok(None),
            Some(i) => {
                let d = PyDict::new(py);
                d.set_item("pid", i.pid)?;
                d.set_item("name", &i.name)?;
                d.set_item("status", status_to_str(i.status))?;
                d.set_item("cpu_usage_percent", i.cpu_usage_percent)?;
                d.set_item("memory_bytes", i.memory_bytes)?;
                d.set_item("restart_count", i.restart_count)?;
                Ok(Some(d))
            }
        }
    }

    /// Return snapshots for all tracked PIDs.
    pub fn list_all<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let all = self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .list_all();

        all.into_iter()
            .map(|i| {
                let d = PyDict::new(py);
                d.set_item("pid", i.pid)?;
                d.set_item("name", &i.name)?;
                d.set_item("status", status_to_str(i.status))?;
                d.set_item("cpu_usage_percent", i.cpu_usage_percent)?;
                d.set_item("memory_bytes", i.memory_bytes)?;
                d.set_item("restart_count", i.restart_count)?;
                Ok(d)
            })
            .collect()
    }

    /// Returns `True` if `pid` is present in the OS process table.
    ///
    /// Performs a fresh OS query for the given PID so `track()` does not need
    /// to be called first.
    pub fn is_alive(&self, pid: u32) -> PyResult<bool> {
        use sysinfo::{Pid, ProcessesToUpdate, System};
        let mut sys = System::new();
        sys.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(pid)]), true);
        Ok(sys.process(Pid::from_u32(pid)).is_some())
    }

    /// Return the number of currently tracked PIDs.
    pub fn tracked_count(&self) -> PyResult<usize> {
        Ok(self
            .inner
            .lock()
            .map_err(|_| PyRuntimeError::new_err("monitor lock poisoned"))?
            .tracked_count())
    }

    pub fn __repr__(&self) -> PyResult<String> {
        let count = self.tracked_count()?;
        Ok(format!("PyProcessMonitor(tracked={count})"))
    }
}

impl Default for PyProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}
