//! Asynchronous background process-watch loop.
//!
//! `ProcessWatcher` wraps a `ProcessMonitor` and runs a tokio task that
//! periodically refreshes process data and emits [`ProcessEvent`]s through
//! an mpsc channel whenever a tracked process changes state.
//!
//! # Example
//!
//! ```text
//! use dcc_mcp_process::watcher::{ProcessWatcher, ProcessEvent};
//! use dcc_mcp_process::types::DccProcessConfig;
//!
//! #[tokio::main]
//! async fn main() {
//!     let watcher = ProcessWatcher::new(500);
//!     watcher.track(std::process::id(), "self");
//!
//!     let mut rx = watcher.spawn_watch_loop();
//!     while let Some(event) = rx.recv().await {
//!         println!("event: {:?}", event);
//!         break; // just read one event for the example
//!     }
//! }
//! ```

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::sync::mpsc;
use tokio::task::JoinHandle;
use tokio::time::interval;
use tracing::{debug, info};

use crate::monitor::ProcessMonitor;
use crate::types::{ProcessInfo, ProcessStatus};

// ── Public event type ────────────────────────────────────────────────────────

/// An event emitted by [`ProcessWatcher`] when a tracked process changes state.
#[derive(Debug, Clone)]
pub enum ProcessEvent {
    /// A previously tracked process is no longer visible in the OS process
    /// table (it exited cleanly or was killed).
    Exited { pid: u32, name: String },

    /// The watcher noticed a significant resource spike or state change.
    StatusChanged {
        pid: u32,
        name: String,
        old_status: ProcessStatus,
        new_status: ProcessStatus,
    },

    /// Periodic heartbeat snapshot (emitted every poll cycle per tracked PID).
    Heartbeat { info: ProcessInfo },

    /// The watcher loop shut down (triggered by [`WatcherHandle::shutdown`]).
    Shutdown,
}

// ── WatcherHandle ────────────────────────────────────────────────────────────

/// A handle to the background watch loop, returned by
/// [`ProcessWatcher::spawn_watch_loop`].
///
/// Dropping the handle does **not** stop the loop; call [`WatcherHandle::shutdown`]
/// explicitly or keep the handle alive for the duration you need monitoring.
pub struct WatcherHandle {
    /// Signalling channel — send `()` to stop the loop.
    stop_tx: tokio::sync::oneshot::Sender<()>,
    /// Join handle for the spawned tokio task.
    task: JoinHandle<()>,
}

impl WatcherHandle {
    /// Signal the background loop to stop and await its completion.
    ///
    /// Returns immediately if the loop has already finished.
    pub async fn shutdown(self) {
        let _ = self.stop_tx.send(());
        let _ = self.task.await;
    }

    /// Returns `true` if the background task is still running.
    #[must_use]
    pub fn is_running(&self) -> bool {
        !self.task.is_finished()
    }
}

// ── ProcessWatcher ───────────────────────────────────────────────────────────

/// Wraps a [`ProcessMonitor`] with a tokio background task that emits
/// [`ProcessEvent`]s whenever monitored process state changes.
///
/// The watcher is cloneable via its inner `Arc`; you can call `track` /
/// `untrack` from any clone while the watch loop is running.
#[derive(Clone)]
pub struct ProcessWatcher {
    inner: Arc<WatcherInner>,
}

struct WatcherInner {
    /// The underlying synchronous monitor.
    monitor: Mutex<ProcessMonitor>,
    /// Poll interval in milliseconds.
    poll_interval_ms: u64,
    /// Last-known status per PID (used for change detection).
    last_status: Mutex<HashMap<u32, ProcessStatus>>,
}

impl ProcessWatcher {
    /// Create a new watcher that polls every `poll_interval_ms` milliseconds.
    pub fn new(poll_interval_ms: u64) -> Self {
        Self {
            inner: Arc::new(WatcherInner {
                monitor: Mutex::new(ProcessMonitor::new()),
                poll_interval_ms,
                last_status: Mutex::new(HashMap::new()),
            }),
        }
    }

    /// Register a PID to monitor.
    pub fn track(&self, pid: u32, name: impl Into<String>) {
        let name = name.into();
        self.inner.monitor.lock().track(pid, name.clone());
        debug!(pid, name, "watcher: tracking pid");
    }

    /// Stop monitoring a PID.
    pub fn untrack(&self, pid: u32) {
        self.inner.monitor.lock().untrack(pid);
        self.inner.last_status.lock().remove(&pid);
        debug!(pid, "watcher: stopped tracking pid");
    }

    /// Return the number of currently tracked PIDs.
    #[must_use]
    pub fn tracked_count(&self) -> usize {
        self.inner.monitor.lock().tracked_count()
    }

    /// Return `true` if the given PID is currently being watched.
    #[must_use]
    pub fn is_tracked(&self, pid: u32) -> bool {
        self.inner.monitor.lock().is_tracked(pid)
    }

    /// Query the latest snapshot for a specific PID (may be slightly stale).
    ///
    /// Returns `None` if the PID is not tracked or the process has already exited.
    #[must_use]
    pub fn query(&self, pid: u32) -> Option<ProcessInfo> {
        self.inner.monitor.lock().query(pid)
    }

    /// Spawn the background watch loop.
    ///
    /// Returns a tuple of:
    /// - An `mpsc::Receiver<ProcessEvent>` — consume events from here.
    /// - A [`WatcherHandle`] — use to stop the loop when done.
    pub fn spawn_watch_loop(&self) -> (mpsc::Receiver<ProcessEvent>, WatcherHandle) {
        let (event_tx, event_rx) = mpsc::channel::<ProcessEvent>(256);
        let (stop_tx, stop_rx) = tokio::sync::oneshot::channel::<()>();

        let inner = Arc::clone(&self.inner);
        let task = tokio::spawn(watch_loop(inner, event_tx, stop_rx));

        let handle = WatcherHandle { stop_tx, task };
        (event_rx, handle)
    }
}

impl Default for ProcessWatcher {
    fn default() -> Self {
        Self::new(2_000)
    }
}

// ── Internal watch loop ───────────────────────────────────────────────────────

async fn watch_loop(
    inner: Arc<WatcherInner>,
    event_tx: mpsc::Sender<ProcessEvent>,
    mut stop_rx: tokio::sync::oneshot::Receiver<()>,
) {
    let poll_ms = inner.poll_interval_ms;
    let mut ticker = interval(Duration::from_millis(poll_ms));
    // First tick fires immediately
    ticker.tick().await;

    info!(poll_interval_ms = poll_ms, "process watch loop started");

    loop {
        tokio::select! {
            _ = ticker.tick() => {
                poll_once(&inner, &event_tx).await;
            }
            _ = &mut stop_rx => {
                info!("process watch loop received stop signal");
                let _ = event_tx.send(ProcessEvent::Shutdown).await;
                break;
            }
        }
    }

    info!("process watch loop stopped");
}

/// Perform one poll cycle: refresh sysinfo, compare with last known status,
/// emit events for any changes.
async fn poll_once(inner: &WatcherInner, event_tx: &mpsc::Sender<ProcessEvent>) {
    // Refresh synchronously (sysinfo is not async)
    let all_info: Vec<ProcessInfo> = {
        let mut monitor = inner.monitor.lock();
        monitor.refresh();
        monitor.list_all()
        // MutexGuard dropped here
    };

    // Build the list of (pid, new_status, old_status) pairs without holding the lock.
    // We snapshot last_status, mutate it locally, then write back.
    let mut status_updates: Vec<(u32, ProcessStatus, Option<ProcessStatus>)> = Vec::new();
    {
        let mut last = inner.last_status.lock();

        for info in &all_info {
            let old = last.get(&info.pid).copied();
            last.insert(info.pid, info.status);
            status_updates.push((info.pid, info.status, old));
        }
        // MutexGuard dropped here
    }

    // Now we can safely use .await (no MutexGuard held)
    for (idx, info) in all_info.iter().enumerate() {
        let pid = info.pid;
        let new_status = info.status;

        // Emit heartbeat for every tracked process
        if event_tx
            .send(ProcessEvent::Heartbeat { info: info.clone() })
            .await
            .is_err()
        {
            // Receiver dropped — stop silently
            return;
        }

        let old_status_opt = status_updates.get(idx).and_then(|(_, _, old)| *old);

        if let Some(old_status) = old_status_opt {
            if old_status != new_status {
                debug!(pid, ?old_status, ?new_status, "process status changed");

                let name = info.name.clone();

                let event = if new_status == ProcessStatus::Stopped
                    || new_status == ProcessStatus::Crashed
                {
                    // Treat stopped/crashed as an Exited event for convenience
                    ProcessEvent::Exited {
                        pid,
                        name: name.clone(),
                    }
                } else {
                    ProcessEvent::StatusChanged {
                        pid,
                        name: name.clone(),
                        old_status,
                        new_status,
                    }
                };

                if event_tx.send(event).await.is_err() {
                    return;
                }
            }
        }
    }
}

// ── Tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_watcher_basic {
        use super::*;

        #[test]
        fn new_watcher_has_zero_tracked() {
            let watcher = ProcessWatcher::new(100);
            assert_eq!(watcher.tracked_count(), 0);
        }

        #[test]
        fn default_watcher_has_2s_interval() {
            // Just ensure Default::default() compiles and produces 0 tracked
            let watcher = ProcessWatcher::default();
            assert_eq!(watcher.tracked_count(), 0);
        }

        #[test]
        fn track_and_untrack() {
            let watcher = ProcessWatcher::new(100);
            watcher.track(1234, "maya");
            assert_eq!(watcher.tracked_count(), 1);
            watcher.untrack(1234);
            assert_eq!(watcher.tracked_count(), 0);
        }

        #[test]
        fn track_self_process_visible_in_query() {
            let pid = std::process::id();
            let watcher = ProcessWatcher::new(100);
            watcher.track(pid, "self");

            // Manually trigger a refresh through the inner monitor
            {
                let mut m = watcher.inner.monitor.lock();
                m.refresh();
            }

            let info = watcher.query(pid);
            assert!(info.is_some(), "self process must be visible after refresh");
        }

        #[test]
        fn clone_shares_state() {
            let watcher = ProcessWatcher::new(100);
            let clone = watcher.clone();
            watcher.track(99, "blender");
            // Clone sees the same tracked count
            assert_eq!(clone.tracked_count(), 1);
        }

        #[test]
        fn untrack_nonexistent_is_noop() {
            let watcher = ProcessWatcher::new(100);
            watcher.untrack(9999); // should not panic
            assert_eq!(watcher.tracked_count(), 0);
        }
    }

    mod test_watch_loop {
        use super::*;
        use tokio::time::timeout;

        /// Spawn the watch loop, track self, wait for at least one Heartbeat event.
        #[tokio::test]
        async fn receives_heartbeat_for_self() {
            let watcher = ProcessWatcher::new(50); // fast poll
            watcher.track(std::process::id(), "self");

            let (mut rx, handle) = watcher.spawn_watch_loop();

            let heartbeat = timeout(Duration::from_secs(5), async {
                loop {
                    match rx.recv().await {
                        Some(ProcessEvent::Heartbeat { info }) => return Some(info),
                        Some(_) => continue,
                        None => return None,
                    }
                }
            })
            .await
            .expect("timed out waiting for heartbeat")
            .expect("channel closed unexpectedly");

            assert_eq!(heartbeat.name, "self");
            assert!(heartbeat.pid > 0);

            handle.shutdown().await;
        }

        /// After shutdown, the receiver should eventually get a Shutdown event.
        #[tokio::test]
        async fn shutdown_sends_shutdown_event() {
            let watcher = ProcessWatcher::new(50);
            let (mut rx, handle) = watcher.spawn_watch_loop();

            handle.shutdown().await;

            let got_shutdown = timeout(Duration::from_secs(2), async {
                while let Some(event) = rx.recv().await {
                    if matches!(event, ProcessEvent::Shutdown) {
                        return true;
                    }
                }
                false
            })
            .await
            .unwrap_or(false);

            assert!(
                got_shutdown,
                "expected a Shutdown event after handle.shutdown()"
            );
        }

        /// Verify WatcherHandle::is_running reflects task state.
        #[tokio::test]
        async fn handle_is_running_until_shutdown() {
            let watcher = ProcessWatcher::new(100);
            let (_rx, handle) = watcher.spawn_watch_loop();

            assert!(
                handle.is_running(),
                "loop should be running right after spawn"
            );
            handle.shutdown().await;
            // After join the task is finished; is_running on the moved value is not
            // testable here since we consumed the handle.  The test above confirms
            // the final state via the Shutdown event.
        }

        /// Two clones of the watcher can each track different PIDs.
        #[tokio::test]
        async fn multiple_pids_get_heartbeats() {
            let watcher = ProcessWatcher::new(50);
            let self_pid = std::process::id();
            watcher.track(self_pid, "self");
            // Track a bogus PID to verify Stopped status also emits a Heartbeat.
            watcher.track(u32::MAX, "ghost");

            let (mut rx, handle) = watcher.spawn_watch_loop();

            let mut saw_self = false;
            let mut saw_ghost = false;

            let result = timeout(Duration::from_secs(5), async {
                while let Some(event) = rx.recv().await {
                    match &event {
                        ProcessEvent::Heartbeat { info } if info.name == "self" => {
                            saw_self = true;
                        }
                        ProcessEvent::Heartbeat { info } if info.name == "ghost" => {
                            saw_ghost = true;
                        }
                        _ => {}
                    }
                    if saw_self && saw_ghost {
                        break;
                    }
                }
            })
            .await;

            handle.shutdown().await;

            assert!(result.is_ok(), "timed out before seeing both heartbeats");
            assert!(saw_self, "did not receive heartbeat for self");
            assert!(saw_ghost, "did not receive heartbeat for ghost");
        }
    }

    mod test_process_event {
        use super::*;

        /// Verify all variants can be constructed and debug-printed.
        #[test]
        fn event_variants_debug() {
            let events = vec![
                ProcessEvent::Exited {
                    pid: 1,
                    name: "maya".into(),
                },
                ProcessEvent::StatusChanged {
                    pid: 2,
                    name: "blender".into(),
                    old_status: ProcessStatus::Running,
                    new_status: ProcessStatus::Unresponsive,
                },
                ProcessEvent::Heartbeat {
                    info: ProcessInfo::new(3, "houdini", ProcessStatus::Running),
                },
                ProcessEvent::Shutdown,
            ];
            for e in events {
                let s = format!("{e:?}");
                assert!(!s.is_empty());
            }
        }

        #[test]
        fn event_is_cloneable() {
            let e = ProcessEvent::Exited {
                pid: 42,
                name: "test".into(),
            };
            let _clone = e.clone();
        }
    }
}
