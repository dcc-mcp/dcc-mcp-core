//! Cross-platform DCC process monitor built on top of `sysinfo`.
//!
//! `ProcessMonitor` lets callers:
//! - Register one or more PIDs to watch.
//! - Query live resource snapshots (`ProcessInfo`).
//! - Check whether a PID is still alive.
//! - Refresh the underlying `sysinfo` system data on demand.

use std::collections::HashMap;

use sysinfo::{Pid, ProcessesToUpdate, System};
use tracing::{debug, warn};

use crate::error::ProcessError;
use crate::types::{ProcessInfo, ProcessStatus};

/// Monitors one or more OS processes and exposes live resource snapshots.
///
/// # Example
///
/// ```text
/// use dcc_mcp_process::monitor::ProcessMonitor;
///
/// let mut monitor = ProcessMonitor::new();
/// monitor.refresh();
/// if let Some(info) = monitor.query(std::process::id()) {
///     println!("self cpu={:.1}%  mem={}B", info.cpu_usage_percent, info.memory_bytes);
/// }
/// ```
pub struct ProcessMonitor {
    system: System,
    /// Map from PID → logical name registered by the caller.
    tracked: HashMap<u32, String>,
}

impl ProcessMonitor {
    /// Create a new monitor with an empty tracked set.
    pub fn new() -> Self {
        Self {
            system: System::new(),
            tracked: HashMap::new(),
        }
    }

    /// Register a PID with a logical name so it appears in `list_all()`.
    ///
    /// If the PID was already registered the name is updated.
    pub fn track(&mut self, pid: u32, name: impl Into<String>) {
        self.tracked.insert(pid, name.into());
        debug!(pid, "tracking process");
    }

    /// Remove a PID from the tracked set.
    pub fn untrack(&mut self, pid: u32) {
        self.tracked.remove(&pid);
        debug!(pid, "stopped tracking process");
    }

    /// Refresh system process data.  Must be called before querying.
    ///
    /// Only processes in the tracked set are fetched from the OS, keeping
    /// overhead proportional to the number of watched processes.
    pub fn refresh(&mut self) {
        if self.tracked.is_empty() {
            // Refresh all processes so `query_by_name` / ad-hoc PIDs work.
            self.system.refresh_processes(ProcessesToUpdate::All, true);
        } else {
            let pids: Vec<Pid> = self.tracked.keys().map(|&p| Pid::from_u32(p)).collect();
            self.system
                .refresh_processes(ProcessesToUpdate::Some(&pids), true);
        }
    }

    /// Return a live `ProcessInfo` snapshot for the given PID, or `None`
    /// if the process cannot be found (already exited or never existed).
    ///
    /// Does **not** call `refresh()` automatically; callers should call
    /// `refresh()` first to ensure the data is up to date.
    #[must_use]
    pub fn query(&self, pid: u32) -> Option<ProcessInfo> {
        let sysinfo_pid = Pid::from_u32(pid);
        let proc = self.system.process(sysinfo_pid)?;

        let name = self
            .tracked
            .get(&pid)
            .cloned()
            .unwrap_or_else(|| proc.name().to_string_lossy().into_owned());

        Some(ProcessInfo {
            pid,
            name,
            status: ProcessStatus::Running,
            cpu_usage_percent: proc.cpu_usage(),
            memory_bytes: proc.memory(),
            restart_count: 0,
        })
    }

    /// Return snapshots for **all** tracked PIDs.
    ///
    /// PIDs that are no longer alive are returned with `ProcessStatus::Stopped`.
    pub fn list_all(&self) -> Vec<ProcessInfo> {
        self.tracked
            .iter()
            .map(|(&pid, name)| {
                self.query(pid).unwrap_or_else(|| {
                    warn!(
                        pid,
                        name, "tracked process not found in sysinfo — marking stopped"
                    );
                    ProcessInfo::new(pid, name.clone(), ProcessStatus::Stopped)
                })
            })
            .collect()
    }

    /// Returns `true` if the given PID is present in the sysinfo process table.
    ///
    /// Call `refresh()` first to obtain up-to-date data.
    #[must_use]
    pub fn is_alive(&self, pid: u32) -> bool {
        self.system.process(Pid::from_u32(pid)).is_some()
    }

    /// Look up the first process whose name contains `name_fragment` (case-insensitive).
    ///
    /// Returns the PID on success, or `ProcessError::NotFound` if nothing matches.
    pub fn find_by_name(&self, name_fragment: &str) -> Result<u32, ProcessError> {
        let lower = name_fragment.to_lowercase();
        for (pid, proc) in self.system.processes() {
            let proc_name = proc.name().to_string_lossy().to_lowercase();
            if proc_name.contains(&lower) {
                return Ok(pid.as_u32());
            }
        }
        Err(ProcessError::NotFound { pid: 0 })
    }

    /// Return the number of currently tracked PIDs.
    #[must_use]
    pub fn tracked_count(&self) -> usize {
        self.tracked.len()
    }

    /// Return `true` if `pid` is currently registered for tracking.
    #[must_use]
    pub fn is_tracked(&self, pid: u32) -> bool {
        self.tracked.contains_key(&pid)
    }
}

impl Default for ProcessMonitor {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_monitor_basic {
        use super::*;

        #[test]
        fn new_monitor_has_zero_tracked() {
            let monitor = ProcessMonitor::new();
            assert_eq!(monitor.tracked_count(), 0);
        }

        #[test]
        fn track_and_untrack() {
            let mut monitor = ProcessMonitor::new();
            monitor.track(1234, "maya");
            assert_eq!(monitor.tracked_count(), 1);
            monitor.untrack(1234);
            assert_eq!(monitor.tracked_count(), 0);
        }

        #[test]
        fn track_updates_name_on_re_register() {
            let mut monitor = ProcessMonitor::new();
            monitor.track(99, "old-name");
            monitor.track(99, "new-name");
            assert_eq!(monitor.tracked_count(), 1);
            assert_eq!(monitor.tracked[&99], "new-name");
        }

        #[test]
        fn untrack_nonexistent_is_noop() {
            let mut monitor = ProcessMonitor::new();
            // Should not panic
            monitor.untrack(9999);
            assert_eq!(monitor.tracked_count(), 0);
        }

        /// Self-process should always be visible after refresh.
        #[test]
        fn query_self_process() {
            let self_pid = std::process::id();
            let mut monitor = ProcessMonitor::new();
            monitor.track(self_pid, "self");
            monitor.refresh();

            let info = monitor
                .query(self_pid)
                .expect("self process must be visible");
            assert_eq!(info.pid, self_pid);
            assert_eq!(info.status, ProcessStatus::Running);
            assert_eq!(info.name, "self");
        }

        #[test]
        fn is_alive_self() {
            let self_pid = std::process::id();
            let mut monitor = ProcessMonitor::new();
            monitor.refresh();
            assert!(monitor.is_alive(self_pid));
        }

        #[test]
        fn is_alive_bogus_pid_returns_false() {
            // PID 0 is never a real user process; PID u32::MAX is extremely unlikely.
            let mut monitor = ProcessMonitor::new();
            monitor.refresh();
            assert!(!monitor.is_alive(u32::MAX));
        }

        #[test]
        fn query_unknown_pid_returns_none() {
            let mut monitor = ProcessMonitor::new();
            monitor.refresh();
            assert!(monitor.query(u32::MAX).is_none());
        }

        #[test]
        fn list_all_marks_dead_pids_stopped() {
            let mut monitor = ProcessMonitor::new();
            // Register a PID that certainly does not exist
            monitor.track(u32::MAX, "ghost");
            monitor.refresh();

            let all = monitor.list_all();
            assert_eq!(all.len(), 1);
            assert_eq!(all[0].status, ProcessStatus::Stopped);
        }

        #[test]
        fn default_is_same_as_new() {
            let monitor = ProcessMonitor::default();
            assert_eq!(monitor.tracked_count(), 0);
        }
    }

    mod test_find_by_name {
        use super::*;

        #[test]
        fn find_bogus_name_returns_not_found() {
            let mut monitor = ProcessMonitor::new();
            monitor.refresh();
            let result = monitor.find_by_name("zzz_nonexistent_dcc_process_zzz");
            assert!(result.is_err());
        }
    }
}
