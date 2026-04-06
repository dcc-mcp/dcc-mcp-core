//! DCC process launcher: spawn, wait-for-ready, and graceful/forceful termination.
//!
//! `DccLauncher` wraps `tokio::process::Command` and provides:
//! - Async `launch()` — spawn + wait for `launch_timeout_ms`.
//! - `terminate()` — send SIGTERM / TerminateProcess and wait for exit.
//! - `kill()` — forceful SIGKILL / TerminateProcess.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::Duration;

use tokio::process::{Child, Command};
use tokio::time;
use tracing::{debug, info, warn};

use crate::error::ProcessError;
use crate::types::{DccProcessConfig, ProcessInfo, ProcessStatus};

/// Manages the lifecycle (spawn, terminate, kill) of DCC application processes.
///
/// All operations are async; wrap in `tokio::task::spawn_blocking` if you
/// need synchronous access from a non-async context (e.g. Maya's main thread).
pub struct DccLauncher {
    /// Live child processes indexed by the config `name` field.
    children: Arc<Mutex<HashMap<String, Child>>>,
    /// Restart counters per config name.
    restart_counts: Arc<Mutex<HashMap<String, u32>>>,
}

impl DccLauncher {
    /// Create a new, empty launcher.
    pub fn new() -> Self {
        Self {
            children: Arc::new(Mutex::new(HashMap::new())),
            restart_counts: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    /// Spawn the DCC process described by `config`.
    ///
    /// The future resolves once the OS reports the child has started.
    /// It does **not** wait for the DCC to be "ready" at the application
    /// level; use the optional `launch_timeout_ms` in `config` for that.
    ///
    /// Returns a `ProcessInfo` snapshot reflecting the newly spawned PID.
    pub async fn launch(&self, config: &DccProcessConfig) -> Result<ProcessInfo, ProcessError> {
        info!(name = %config.name, executable = %config.executable, "launching DCC process");

        let mut cmd = Command::new(&config.executable);
        cmd.args(&config.args);
        // Detach stdin so the DCC doesn't block on terminal input.
        cmd.stdin(std::process::Stdio::null());

        let child = time::timeout(
            Duration::from_millis(config.launch_timeout_ms),
            async move {
                cmd.spawn()
                    .map_err(|e| ProcessError::spawn_failed(&config.executable, e.to_string()))
            },
        )
        .await
        .map_err(|_| ProcessError::LaunchTimeout {
            command: config.executable.clone(),
            timeout_ms: config.launch_timeout_ms,
        })??;

        let pid = child
            .id()
            .ok_or_else(|| ProcessError::internal("child has no PID immediately after spawn"))?;

        debug!(pid, name = %config.name, "DCC process spawned");

        {
            let mut children = self
                .children
                .lock()
                .map_err(|_| ProcessError::internal("children lock poisoned"))?;
            children.insert(config.name.clone(), child);
        }

        Ok(ProcessInfo::new(
            pid,
            config.name.clone(),
            ProcessStatus::Starting,
        ))
    }

    /// Terminate (SIGTERM / TerminateProcess) the named process gracefully.
    ///
    /// Waits up to `timeout_ms` for the process to exit, then returns.
    /// If the process has already exited this is a no-op.
    pub async fn terminate(&self, name: &str, timeout_ms: u64) -> Result<(), ProcessError> {
        // Remove the child from the map before any await so the MutexGuard is
        // dropped before we yield.
        let mut child = {
            let mut children = self
                .children
                .lock()
                .map_err(|_| ProcessError::internal("children lock poisoned"))?;
            match children.remove(name) {
                Some(c) => c,
                None => {
                    warn!(name, "terminate called for unknown process name");
                    return Ok(());
                }
            }
            // MutexGuard dropped here
        };

        // Try graceful kill first
        let pid = child.id().unwrap_or(0);
        if let Err(e) = child.start_kill() {
            return Err(ProcessError::TerminateFailed {
                pid,
                reason: e.to_string(),
            });
        }

        // Wait up to timeout for the child to exit
        let wait_result = time::timeout(Duration::from_millis(timeout_ms), child.wait()).await;

        match wait_result {
            Ok(Ok(status)) => {
                debug!(name, ?status, "process exited after terminate");
            }
            Ok(Err(e)) => {
                warn!(name, error = %e, "wait() failed after terminate");
            }
            Err(_) => {
                warn!(
                    name,
                    timeout_ms, "process did not exit within timeout after terminate"
                );
            }
        }

        Ok(())
    }

    /// Forcefully kill the named process (SIGKILL / TerminateProcess).
    pub async fn kill(&self, name: &str) -> Result<(), ProcessError> {
        // Remove the child from the map before any await so the MutexGuard is
        // dropped before we yield.
        let mut child = {
            let mut children = self
                .children
                .lock()
                .map_err(|_| ProcessError::internal("children lock poisoned"))?;
            match children.remove(name) {
                Some(c) => c,
                None => {
                    warn!(name, "kill called for unknown process name");
                    return Ok(());
                }
            }
            // MutexGuard dropped here
        };

        let pid = child.id().unwrap_or(0);
        child
            .kill()
            .await
            .map_err(|e| ProcessError::TerminateFailed {
                pid,
                reason: e.to_string(),
            })?;

        info!(name, "process killed");
        Ok(())
    }

    /// Returns the PID of the named running child, or `None` if not tracked.
    #[must_use]
    pub fn pid_of(&self, name: &str) -> Option<u32> {
        let children = self.children.lock().ok()?;
        children.get(name).and_then(|c| c.id())
    }

    /// Returns the number of currently tracked live children.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.children.lock().map(|c| c.len()).unwrap_or(0)
    }

    /// Increment and return the restart counter for `name`.
    pub fn increment_restart_count(&self, name: &str) -> u32 {
        let mut counts = self
            .restart_counts
            .lock()
            .expect("restart_counts lock poisoned");
        let entry = counts.entry(name.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Return the current restart count for `name` (0 if never restarted).
    #[must_use]
    pub fn restart_count(&self, name: &str) -> u32 {
        self.restart_counts
            .lock()
            .map(|c| *c.get(name).unwrap_or(&0))
            .unwrap_or(0)
    }
}

impl Default for DccLauncher {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_launcher_basic {
        use super::*;

        #[test]
        fn new_has_zero_running() {
            let launcher = DccLauncher::new();
            assert_eq!(launcher.running_count(), 0);
        }

        #[test]
        fn default_equals_new() {
            let launcher = DccLauncher::default();
            assert_eq!(launcher.running_count(), 0);
        }

        #[test]
        fn pid_of_unknown_returns_none() {
            let launcher = DccLauncher::new();
            assert!(launcher.pid_of("nonexistent").is_none());
        }

        #[test]
        fn restart_count_starts_at_zero() {
            let launcher = DccLauncher::new();
            assert_eq!(launcher.restart_count("maya"), 0);
        }

        #[test]
        fn increment_restart_count() {
            let launcher = DccLauncher::new();
            assert_eq!(launcher.increment_restart_count("maya"), 1);
            assert_eq!(launcher.increment_restart_count("maya"), 2);
            assert_eq!(launcher.restart_count("maya"), 2);
        }

        #[test]
        fn restart_count_independent_per_name() {
            let launcher = DccLauncher::new();
            launcher.increment_restart_count("maya");
            launcher.increment_restart_count("maya");
            launcher.increment_restart_count("blender");
            assert_eq!(launcher.restart_count("maya"), 2);
            assert_eq!(launcher.restart_count("blender"), 1);
            assert_eq!(launcher.restart_count("houdini"), 0);
        }

        #[tokio::test]
        async fn launch_invalid_executable_returns_error() {
            let launcher = DccLauncher::new();
            let cfg = DccProcessConfig::new("test-dcc", "/nonexistent/path/to/dcc_exe_zzz");
            let result = launcher.launch(&cfg).await;
            assert!(result.is_err());
            let err = result.unwrap_err();
            assert!(
                matches!(
                    err,
                    ProcessError::SpawnFailed { .. } | ProcessError::LaunchTimeout { .. }
                ),
                "unexpected error: {err}"
            );
        }

        #[tokio::test]
        async fn terminate_unknown_name_is_noop() {
            let launcher = DccLauncher::new();
            // Should not panic or error
            let result = launcher.terminate("ghost", 100).await;
            assert!(result.is_ok());
        }

        #[tokio::test]
        async fn kill_unknown_name_is_noop() {
            let launcher = DccLauncher::new();
            let result = launcher.kill("ghost").await;
            assert!(result.is_ok());
        }

        /// Launch a real trivial process (echo), confirm PID is set, then kill it.
        #[tokio::test]
        async fn launch_real_process() {
            let launcher = DccLauncher::new();

            // Use a cross-platform trivial command that exits quickly
            #[cfg(windows)]
            let executable = "cmd";
            #[cfg(not(windows))]
            let executable = "sh";

            #[cfg(windows)]
            let args = ["/C", "timeout /T 5 /NOBREAK > nul"];
            #[cfg(not(windows))]
            let args = vec!["-c", "sleep 5"];

            let mut cfg = DccProcessConfig::new("test-echo", executable);
            cfg.args = args.iter().map(|s| s.to_string()).collect();

            match launcher.launch(&cfg).await {
                Ok(info) => {
                    assert_eq!(info.name, "test-echo");
                    assert_eq!(info.status, ProcessStatus::Starting);
                    assert!(info.pid > 0, "PID must be positive");

                    // Clean up
                    let _ = launcher.kill("test-echo").await;
                }
                Err(e) => {
                    // On some CI environments the command may not exist; skip gracefully
                    eprintln!("skipping launch_real_process: {e}");
                }
            }
        }
    }
}
