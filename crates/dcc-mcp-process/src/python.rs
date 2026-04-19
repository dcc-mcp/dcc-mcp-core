//! PyO3 bindings for the `dcc-mcp-process` crate.
//!
//! Exposes `ProcessMonitor`, `DccLauncher`, `CrashRecoveryPolicy`, and
//! `ProcessWatcher` to Python as:
//!
//! ```text
//! from dcc_mcp_core import (
//!     PyProcessMonitor,
//!     PyDccLauncher,
//!     PyCrashRecoveryPolicy,
//!     PyProcessWatcher,
//! )
//! ```

use pyo3::exceptions::{PyRuntimeError, PyValueError};
use pyo3::prelude::*;
use pyo3::types::PyDict;
use std::sync::{Arc, Mutex};
use tokio::runtime::Runtime;

use crate::dispatcher::{
    ActionOutcome, HostDispatcher, JobRequest, StandaloneDispatcher, ThreadAffinity,
};
use crate::error::ProcessError;
use crate::launcher::DccLauncher;
use crate::monitor::ProcessMonitor;
use crate::pump::PumpedDispatcher;
use crate::recovery::{BackoffStrategy, CrashRecoveryPolicy};
use crate::types::{DccProcessConfig, ProcessStatus};
use crate::watcher::ProcessWatcher;

// ── Internal helpers ─────────────────────────────────────────────────────────

fn runtime() -> PyResult<Arc<Runtime>> {
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

fn map_process_err(e: ProcessError) -> PyErr {
    PyRuntimeError::new_err(e.to_string())
}

/// Convert `ProcessStatus` to its string representation.
fn status_to_str(s: ProcessStatus) -> &'static str {
    match s {
        ProcessStatus::Running => "running",
        ProcessStatus::Starting => "starting",
        ProcessStatus::Stopped => "stopped",
        ProcessStatus::Crashed => "crashed",
        ProcessStatus::Unresponsive => "unresponsive",
        ProcessStatus::Restarting => "restarting",
    }
}

// ── PyProcessMonitor ─────────────────────────────────────────────────────────

/// Cross-platform DCC process monitor.
///
/// Wraps `ProcessMonitor` — tracks live resource snapshots via `sysinfo`.
///
/// Example::
///
///     mon = PyProcessMonitor()
///     import os
///     mon.track(os.getpid(), "self")
///     mon.refresh()
///     info = mon.query(os.getpid())
///     print(info["status"])  # "running"
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

// ── PyDccLauncher ─────────────────────────────────────────────────────────────

/// Async DCC process launcher (spawn / terminate / kill).
///
/// Example::
///
///     launcher = PyDccLauncher()
///     # info = launcher.launch("maya-2025", "/usr/autodesk/maya/bin/maya", [], 30000)
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

// ── PyCrashRecoveryPolicy ────────────────────────────────────────────────────

/// Crash recovery policy for DCC processes.
///
/// Example::
///
///     policy = PyCrashRecoveryPolicy(max_restarts=3)
///     policy.use_exponential_backoff(initial_ms=1000, max_delay_ms=30000)
///     print(policy.should_restart("crashed"))   # True
///     print(policy.next_delay_ms("maya", 0))    # 1000
#[pyclass(name = "PyCrashRecoveryPolicy")]
pub struct PyCrashRecoveryPolicy {
    inner: CrashRecoveryPolicy,
}

#[pymethods]
impl PyCrashRecoveryPolicy {
    /// Create a policy with ``max_restarts`` and fixed 2 s back-off by default.
    #[new]
    #[pyo3(signature = (max_restarts=3))]
    pub fn new(max_restarts: u32) -> Self {
        Self {
            inner: CrashRecoveryPolicy::new(max_restarts),
        }
    }

    /// Switch to exponential back-off.
    pub fn use_exponential_backoff(&mut self, initial_ms: u64, max_delay_ms: u64) {
        self.inner.backoff = BackoffStrategy::Exponential {
            initial_ms,
            max_delay_ms,
        };
    }

    /// Switch to fixed back-off.
    pub fn use_fixed_backoff(&mut self, delay_ms: u64) {
        self.inner.backoff = BackoffStrategy::Fixed { delay_ms };
    }

    /// Returns `True` if the given status string warrants a restart.
    ///
    /// Recognised status values: ``"crashed"``, ``"unresponsive"``.
    /// Always returns `False` when `max_restarts` is 0.
    pub fn should_restart(&self, status: &str) -> PyResult<bool> {
        if self.inner.max_restarts == 0 {
            return Ok(false);
        }
        let s = parse_status(status)?;
        Ok(self.inner.should_restart(s))
    }

    /// Return the delay (ms) before attempt ``attempt`` (0-indexed), or raise
    /// `RuntimeError` if `max_restarts` has been exceeded.
    pub fn next_delay_ms(&self, name: &str, attempt: u32) -> PyResult<u64> {
        let cfg = DccProcessConfig::new(name, "dummy");
        self.inner
            .next_restart_delay(&cfg, attempt)
            .map(|d| d.as_millis() as u64)
            .map_err(map_process_err)
    }

    /// Maximum number of restart attempts.
    #[getter]
    pub fn max_restarts(&self) -> u32 {
        self.inner.max_restarts
    }

    pub fn __repr__(&self) -> String {
        format!(
            "PyCrashRecoveryPolicy(max_restarts={})",
            self.inner.max_restarts
        )
    }
}

fn parse_status(s: &str) -> PyResult<ProcessStatus> {
    match s {
        "running" => Ok(ProcessStatus::Running),
        "starting" => Ok(ProcessStatus::Starting),
        "stopped" => Ok(ProcessStatus::Stopped),
        "crashed" => Ok(ProcessStatus::Crashed),
        "unresponsive" => Ok(ProcessStatus::Unresponsive),
        "restarting" => Ok(ProcessStatus::Restarting),
        other => Err(PyValueError::new_err(format!(
            "unknown ProcessStatus: '{other}' — expected one of running/starting/stopped/crashed/unresponsive/restarting"
        ))),
    }
}

// ── PyProcessWatcher ─────────────────────────────────────────────────────────

/// Asynchronous background process watcher with event polling.
///
/// Spawns a background loop that periodically polls tracked processes and
/// collects events.  Python consumers can call `poll_events()` to drain the
/// event queue.
///
/// Example::
///
///     import os, time
///     watcher = PyProcessWatcher(poll_interval_ms=200)
///     watcher.track(os.getpid(), "self")
///     watcher.start()
///     time.sleep(0.5)
///     events = watcher.poll_events()
///     print(events[0]["type"])  # "heartbeat"
///     watcher.stop()
#[pyclass(name = "PyProcessWatcher")]
pub struct PyProcessWatcher {
    watcher: ProcessWatcher,
    /// Shared event queue — the background loop pushes events here.
    event_queue: Arc<Mutex<Vec<PyWatcherEvent>>>,
    /// Handle to the background task (if running).
    handle_cell: Arc<Mutex<Option<BackgroundHandle>>>,
}

/// Lightweight internal struct that holds the WatcherHandle + drain task.
struct BackgroundHandle {
    watcher_handle: crate::watcher::WatcherHandle,
    drain_task: tokio::task::JoinHandle<()>,
}

/// Serialisable event type for Python consumers.
#[derive(Debug, Clone)]
struct PyWatcherEvent {
    kind: String,
    pid: u32,
    name: String,
    old_status: Option<String>,
    new_status: Option<String>,
    cpu_usage_percent: Option<f32>,
    memory_bytes: Option<u64>,
}

impl PyWatcherEvent {
    fn to_dict<'py>(&self, py: Python<'py>) -> PyResult<Bound<'py, PyDict>> {
        let d = PyDict::new(py);
        d.set_item("type", &self.kind)?;
        d.set_item("pid", self.pid)?;
        d.set_item("name", &self.name)?;
        if let Some(ref s) = self.old_status {
            d.set_item("old_status", s)?;
        }
        if let Some(ref s) = self.new_status {
            d.set_item("new_status", s)?;
        }
        if let Some(cpu) = self.cpu_usage_percent {
            d.set_item("cpu_usage_percent", cpu)?;
        }
        if let Some(mem) = self.memory_bytes {
            d.set_item("memory_bytes", mem)?;
        }
        Ok(d)
    }
}

#[pymethods]
impl PyProcessWatcher {
    /// Create a new watcher that polls every `poll_interval_ms` milliseconds.
    #[new]
    #[pyo3(signature = (poll_interval_ms=500))]
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            watcher: ProcessWatcher::new(poll_interval_ms),
            event_queue: Arc::new(Mutex::new(Vec::new())),
            handle_cell: Arc::new(Mutex::new(None)),
        }
    }

    /// Register a PID to monitor.
    pub fn track(&self, pid: u32, name: &str) {
        self.watcher.track(pid, name);
    }

    /// Stop monitoring a PID.
    pub fn untrack(&self, pid: u32) {
        self.watcher.untrack(pid);
    }

    /// Add a PID to watch.  Alias for `track`.
    pub fn add_watch(&self, pid: u32, name: &str) {
        self.watcher.track(pid, name);
    }

    /// Remove a PID from watching.  Alias for `untrack`.
    pub fn remove_watch(&self, pid: u32) {
        self.watcher.untrack(pid);
    }

    /// Number of currently watched PIDs.  Alias for `tracked_count`.
    pub fn watch_count(&self) -> usize {
        self.watcher.tracked_count()
    }

    /// Return `True` if the given PID is currently being watched.
    pub fn is_watched(&self, pid: u32) -> bool {
        self.watcher.is_tracked(pid)
    }

    /// Start the background watch loop.  No-op if already running.
    pub fn start(&self) -> PyResult<()> {
        let mut cell = self
            .handle_cell
            .lock()
            .map_err(|_| PyRuntimeError::new_err("handle_cell lock poisoned"))?;

        if cell.is_some() {
            return Ok(()); // already running
        }

        let rt = runtime()?;

        // `spawn_watch_loop` calls `tokio::spawn` internally, which requires a
        // Tokio runtime context. Use `block_on` to enter the runtime and run
        // an async block that calls `spawn_watch_loop` from inside it.
        let queue_clone = Arc::clone(&self.event_queue);
        let watcher_clone = self.watcher.clone();

        let (watcher_handle, drain_task) = rt.block_on(async move {
            let (mut rx, watcher_handle) = watcher_clone.spawn_watch_loop();

            use crate::watcher::ProcessEvent;
            let drain_task = tokio::spawn(async move {
                while let Some(event) = rx.recv().await {
                    let py_event = match event {
                        ProcessEvent::Heartbeat { info } => PyWatcherEvent {
                            kind: "heartbeat".into(),
                            pid: info.pid,
                            name: info.name.clone(),
                            old_status: None,
                            new_status: Some(status_to_str(info.status).to_string()),
                            cpu_usage_percent: Some(info.cpu_usage_percent),
                            memory_bytes: Some(info.memory_bytes),
                        },
                        ProcessEvent::StatusChanged {
                            pid,
                            name,
                            old_status,
                            new_status,
                        } => PyWatcherEvent {
                            kind: "status_changed".into(),
                            pid,
                            name,
                            old_status: Some(status_to_str(old_status).to_string()),
                            new_status: Some(status_to_str(new_status).to_string()),
                            cpu_usage_percent: None,
                            memory_bytes: None,
                        },
                        ProcessEvent::Exited { pid, name } => PyWatcherEvent {
                            kind: "exited".into(),
                            pid,
                            name,
                            old_status: None,
                            new_status: None,
                            cpu_usage_percent: None,
                            memory_bytes: None,
                        },
                        ProcessEvent::Shutdown => return,
                    };
                    if let Ok(mut q) = queue_clone.lock() {
                        q.push(py_event);
                    }
                }
            });
            (watcher_handle, drain_task)
        });

        *cell = Some(BackgroundHandle {
            watcher_handle,
            drain_task,
        });

        Ok(())
    }

    /// Stop the background watch loop.  No-op if not running.
    pub fn stop(&self) -> PyResult<()> {
        let handle = {
            let mut cell = self
                .handle_cell
                .lock()
                .map_err(|_| PyRuntimeError::new_err("handle_cell lock poisoned"))?;
            cell.take()
        };

        if let Some(h) = handle {
            let rt = runtime()?;
            // Shut down the watcher loop (sends Shutdown event to channel)
            rt.block_on(h.watcher_handle.shutdown());
            // Wait for drain task to finish processing remaining events
            let _ = rt.block_on(h.drain_task);
        }

        Ok(())
    }

    /// Drain and return all pending events as a list of dicts.
    pub fn poll_events<'py>(&self, py: Python<'py>) -> PyResult<Vec<Bound<'py, PyDict>>> {
        let events = {
            let mut q = self
                .event_queue
                .lock()
                .map_err(|_| PyRuntimeError::new_err("event_queue lock poisoned"))?;
            std::mem::take(&mut *q)
        };

        events.iter().map(|e| e.to_dict(py)).collect()
    }

    /// Returns `True` if the background loop is running.
    pub fn is_running(&self) -> PyResult<bool> {
        let cell = self
            .handle_cell
            .lock()
            .map_err(|_| PyRuntimeError::new_err("handle_cell lock poisoned"))?;
        Ok(cell.is_some())
    }

    /// Number of currently tracked PIDs.
    pub fn tracked_count(&self) -> usize {
        self.watcher.tracked_count()
    }

    pub fn __repr__(&self) -> PyResult<String> {
        let running = self.is_running()?;
        let tracked = self.tracked_count();
        Ok(format!(
            "PyProcessWatcher(running={running}, tracked={tracked})"
        ))
    }
}

/// Register all process Python classes on the given module.
pub fn register_classes(m: &Bound<'_, PyModule>) -> PyResult<()> {
    m.add_class::<PyProcessMonitor>()?;
    m.add_class::<PyDccLauncher>()?;
    m.add_class::<PyCrashRecoveryPolicy>()?;
    m.add_class::<PyProcessWatcher>()?;
    m.add_class::<PyStandaloneDispatcher>()?;
    m.add_class::<PyPumpedDispatcher>()?;
    Ok(())
}

// ── PyStandaloneDispatcher ───────────────────────────────────────────────────

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

// ── PyPumpedDispatcher ───────────────────────────────────────────────────────

/// Thread-affinity aware dispatcher with cooperative main-thread pump.
///
/// Combines an ``ipckit::MainThreadPump`` for ``"main"``/``"named"`` affinity
/// jobs with Tokio worker execution for ``"any"`` affinity jobs.
///
/// Call :meth:`pump` from the DCC host's idle callback (e.g. Maya
/// ``scriptJob(idleEvent=...)``) to drain pending main-thread work items.
///
/// Example::
///
///     from dcc_mcp_core import PumpedDispatcher
///
///     dispatcher = PumpedDispatcher(budget_ms=8)
///     result = dispatcher.submit("update-ui", payload="refresh", affinity="main")
///     # In idle callback:
///     stats = dispatcher.pump()
///     print(f"processed={stats['processed']}, remaining={stats['remaining']}")
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
