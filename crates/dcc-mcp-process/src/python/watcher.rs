//! `PyProcessWatcher` — async background process watcher with event polling.
//!
//! Spawns a Tokio background loop that periodically polls tracked
//! processes and pushes [`ProcessEvent`](crate::watcher::ProcessEvent)s
//! into a shared queue; Python consumers drain the queue through
//! [`PyProcessWatcher::poll_events`].

use std::sync::{Arc, Mutex};

use pyo3::exceptions::PyRuntimeError;
use pyo3::prelude::*;
use pyo3::types::PyDict;
#[cfg(feature = "stub-gen")]
use pyo3_stub_gen_derive::{gen_stub_pyclass, gen_stub_pymethods};

use super::helpers::{runtime, status_to_str};
use crate::watcher::ProcessWatcher;

/// Asynchronous background process watcher with event polling.
///
/// Spawns a background loop that periodically polls tracked processes and
/// collects events.  Python consumers can call `poll_events()` to drain the
/// event queue.
///
/// # Example (Python)
///
/// ```python
/// import os, time
/// watcher = PyProcessWatcher(poll_interval_ms=200)
/// watcher.track(os.getpid(), "self")
/// watcher.start()
/// time.sleep(0.5)
/// events = watcher.poll_events()
/// print(events[0]["type"])  # "heartbeat"
/// watcher.stop()
/// ```
#[cfg_attr(feature = "stub-gen", gen_stub_pyclass)]
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

#[cfg_attr(feature = "stub-gen", gen_stub_pymethods)]
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
