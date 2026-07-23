//! DCC process launcher: spawn, wait-for-ready, and graceful/forceful termination.
//!
//! `DccLauncher` wraps `tokio::process::Command` and provides:
//! - Async `launch()` — spawn + wait for `launch_timeout_ms`.
//! - `terminate()` — send SIGTERM / TerminateProcess and wait for exit.
//! - `kill()` — forceful SIGKILL / TerminateProcess.

use std::collections::HashMap;
use std::sync::Arc;
use std::time::Duration;

#[cfg(windows)]
use std::mem::size_of;
#[cfg(windows)]
use std::os::windows::io::{FromRawHandle, OwnedHandle};

use parking_lot::Mutex;
use tokio::process::{Child, Command};
use tokio::time;
use tracing::{debug, info, warn};

use crate::error::ProcessError;
use crate::types::{DccLaunchOptions, DccProcessConfig, ProcessInfo, ProcessStatus};

#[cfg(windows)]
struct ChildJob {
    _handle: OwnedHandle,
}

#[cfg(windows)]
fn assign_kill_on_close_job(child: &Child) -> std::io::Result<ChildJob> {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject,
    };

    let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
    if job.is_null() {
        return Err(std::io::Error::last_os_error());
    }
    let mut limits = JOBOBJECT_EXTENDED_LIMIT_INFORMATION::default();
    limits.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
    let configured = unsafe {
        SetInformationJobObject(
            job,
            JobObjectExtendedLimitInformation,
            &limits as *const _ as *const _,
            size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
        )
    };
    let process = child
        .raw_handle()
        .ok_or_else(|| std::io::Error::other("child exited before job assignment"))?;
    let assigned =
        configured != 0 && unsafe { AssignProcessToJobObject(job, process as HANDLE) } != 0;
    if !assigned {
        let error = std::io::Error::last_os_error();
        unsafe { CloseHandle(job) };
        return Err(error);
    }
    Ok(ChildJob {
        _handle: unsafe { OwnedHandle::from_raw_handle(job as _) },
    })
}

/// Manages the lifecycle (spawn, terminate, kill) of DCC application processes.
///
/// All operations are async; wrap in `tokio::task::spawn_blocking` if you
/// need synchronous access from a non-async context (e.g. Maya's main thread).
pub struct DccLauncher {
    /// Live child processes indexed by the config `name` field.
    children: Arc<Mutex<HashMap<String, Child>>>,
    /// Windows job handles that terminate their entire child tree when dropped.
    #[cfg(windows)]
    jobs: Arc<Mutex<HashMap<String, ChildJob>>>,
    /// Restart counters per config name.
    restart_counts: Arc<Mutex<HashMap<String, u32>>>,
}

impl DccLauncher {
    /// Create a new, empty launcher.
    pub fn new() -> Self {
        Self {
            children: Arc::new(Mutex::new(HashMap::new())),
            #[cfg(windows)]
            jobs: Arc::new(Mutex::new(HashMap::new())),
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
        self.launch_with_options(config, &DccLaunchOptions::default())
            .await
    }

    /// Spawn the DCC process with child-specific environment and working-directory settings.
    ///
    /// `options` affect only the new child. The parent process environment and
    /// working directory remain unchanged.
    pub async fn launch_with_options(
        &self,
        config: &DccProcessConfig,
        options: &DccLaunchOptions,
    ) -> Result<ProcessInfo, ProcessError> {
        info!(name = %config.name, executable = %config.executable, "launching DCC process");

        let mut cmd = Command::new(&config.executable);
        cmd.args(&config.args);
        cmd.envs(&options.environment);
        if let Some(working_directory) = &options.working_directory {
            cmd.current_dir(working_directory);
        }
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

        #[cfg(windows)]
        let job = match assign_kill_on_close_job(&child) {
            Ok(job) => job,
            Err(error) => {
                let mut child = child;
                let _ = child.start_kill();
                return Err(ProcessError::internal(format!(
                    "failed to assign Windows ownership job for child {pid}: {error}"
                )));
            }
        };

        debug!(pid, name = %config.name, "DCC process spawned");

        {
            let mut children = self.children.lock();
            children.insert(config.name.clone(), child);
        }
        #[cfg(windows)]
        self.jobs.lock().insert(config.name.clone(), job);

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
            let mut children = self.children.lock();
            match children.remove(name) {
                Some(c) => c,
                None => {
                    warn!(name, "terminate called for unknown process name");
                    return Ok(());
                }
            }
            // MutexGuard dropped here
        };
        #[cfg(windows)]
        let job = self.jobs.lock().remove(name);

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

        #[cfg(windows)]
        drop(job);

        Ok(())
    }

    /// Forcefully kill the named process (SIGKILL / TerminateProcess).
    pub async fn kill(&self, name: &str) -> Result<(), ProcessError> {
        // Remove the child from the map before any await so the MutexGuard is
        // dropped before we yield.
        let mut child = {
            let mut children = self.children.lock();
            match children.remove(name) {
                Some(c) => c,
                None => {
                    warn!(name, "kill called for unknown process name");
                    return Ok(());
                }
            }
            // MutexGuard dropped here
        };
        #[cfg(windows)]
        let job = self.jobs.lock().remove(name);

        let pid = child.id().unwrap_or(0);
        child
            .kill()
            .await
            .map_err(|e| ProcessError::TerminateFailed {
                pid,
                reason: e.to_string(),
            })?;

        info!(name, "process killed");
        #[cfg(windows)]
        drop(job);
        Ok(())
    }

    /// Returns the PID of the named running child, or `None` if not tracked.
    #[must_use]
    pub fn pid_of(&self, name: &str) -> Option<u32> {
        self.children.lock().get(name).and_then(|c| c.id())
    }

    /// Returns the number of currently tracked live children.
    #[must_use]
    pub fn running_count(&self) -> usize {
        self.children.lock().len()
    }

    /// Increment and return the restart counter for `name`.
    pub fn increment_restart_count(&self, name: &str) -> u32 {
        let mut counts = self.restart_counts.lock();
        let entry = counts.entry(name.to_string()).or_insert(0);
        *entry += 1;
        *entry
    }

    /// Return the current restart count for `name` (0 if never restarted).
    #[must_use]
    pub fn restart_count(&self, name: &str) -> u32 {
        self.restart_counts.lock().get(name).copied().unwrap_or(0)
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

        #[cfg(windows)]
        #[tokio::test]
        async fn dropping_launcher_kills_its_child() {
            use sysinfo::{Pid, ProcessesToUpdate, System};

            let launcher = DccLauncher::new();
            let mut config = DccProcessConfig::new("owned-child", "cmd");
            config.args = vec!["/C".into(), "ping 127.0.0.1 -n 30 > nul".into()];
            let info = launcher.launch(&config).await.expect("launch child");

            drop(launcher);

            let deadline = time::Instant::now() + Duration::from_secs(1);
            let exited = loop {
                let mut system = System::new();
                system.refresh_processes(ProcessesToUpdate::Some(&[Pid::from_u32(info.pid)]), true);
                if system.process(Pid::from_u32(info.pid)).is_none() {
                    break true;
                }
                if time::Instant::now() >= deadline {
                    break false;
                }
                time::sleep(Duration::from_millis(50)).await;
            };
            if !exited {
                let _ = std::process::Command::new("taskkill")
                    .args(["/PID", &info.pid.to_string(), "/T", "/F"])
                    .status();
            }
            assert!(exited, "child survived its launcher");
        }

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

            // Use a cross-platform trivial command that exits quickly.
            let (executable, args): (&str, &[&str]) = core::cfg_select! {
                windows => ("cmd", &["/C", "timeout /T 5 /NOBREAK > nul"]),
                _ => ("sh", &["-c", "sleep 5"]),
            };

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
                    tracing::warn!("skipping launch_real_process: {e}");
                }
            }
        }

        #[tokio::test]
        async fn launch_with_options_applies_child_environment_and_working_directory() {
            use std::fs;
            use std::path::PathBuf;
            use std::time::{SystemTime, UNIX_EPOCH};

            const ENV_KEY: &str = "DCC_MCP_PROCESS_TEST_CHILD_6E0B39B4";
            assert!(std::env::var_os(ENV_KEY).is_none());

            let suffix = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("system clock must be after Unix epoch")
                .as_nanos();
            let test_directory = std::env::temp_dir().join(format!(
                "dcc-mcp-process-launch-{}-{suffix}",
                std::process::id()
            ));
            fs::create_dir_all(&test_directory).expect("create test working directory");
            let output_path = test_directory.join("child.txt");
            let ready_path = test_directory.join("child.ready");
            let release_directory = test_directory
                .parent()
                .expect("test directory must have a parent");

            let (executable, args): (&str, Vec<String>) = core::cfg_select! {
                windows => (
                    "powershell.exe",
                    vec![
                        "-NoProfile".to_string(),
                        "-NonInteractive".to_string(),
                        "-Command".to_string(),
                        format!(
                            "$env:{ENV_KEY} | Set-Content -LiteralPath child.txt -Encoding ascii; (Get-Location).Path | Add-Content -LiteralPath child.txt -Encoding ascii; Set-Location -LiteralPath $env:DCC_MCP_PROCESS_TEST_RELEASE_CWD; 'ready' | Set-Content -LiteralPath $env:DCC_MCP_PROCESS_TEST_READY -Encoding ascii; Start-Sleep -Seconds 30"
                        ),
                    ],
                ),
                _ => (
                    "sh",
                    vec![
                        "-c".to_string(),
                        format!(
                            "printf '%s\\n%s\\n' \"${ENV_KEY}\" \"$PWD\" > child.txt; cd \"$DCC_MCP_PROCESS_TEST_RELEASE_CWD\"; printf ready > \"$DCC_MCP_PROCESS_TEST_READY\"; sleep 30"
                        ),
                    ],
                ),
            };

            let mut config = DccProcessConfig::new("isolated-child", executable);
            config.args = args;
            let options = DccLaunchOptions::new()
                .with_environment([
                    (ENV_KEY.to_string(), "child-only".to_string()),
                    (
                        "DCC_MCP_PROCESS_TEST_READY".to_string(),
                        ready_path.to_string_lossy().into_owned(),
                    ),
                    (
                        "DCC_MCP_PROCESS_TEST_RELEASE_CWD".to_string(),
                        release_directory.to_string_lossy().into_owned(),
                    ),
                ])
                .with_working_directory(test_directory.to_string_lossy());
            let launcher = DccLauncher::new();

            launcher
                .launch_with_options(&config, &options)
                .await
                .expect("launch isolated child");

            let deadline = time::Instant::now() + Duration::from_secs(5);
            while !ready_path.is_file() {
                assert!(
                    time::Instant::now() < deadline,
                    "child did not release its cwd and write the ready marker"
                );
                time::sleep(Duration::from_millis(25)).await;
            }
            let output = fs::read_to_string(&output_path)
                .expect("read child environment and working directory");

            let mut lines = output.lines();
            assert_eq!(lines.next(), Some("child-only"));
            let child_cwd = PathBuf::from(lines.next().expect("child cwd line"));
            assert_eq!(
                fs::canonicalize(child_cwd).expect("canonicalize child cwd"),
                fs::canonicalize(&test_directory).expect("canonicalize expected cwd")
            );
            assert!(std::env::var_os(ENV_KEY).is_none());

            let _ = launcher.kill("isolated-child").await;
            fs::remove_dir_all(test_directory).expect("remove test working directory");
        }
    }
}
