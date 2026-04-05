//! Core data types for DCC process management.

use serde::{Deserialize, Serialize};

/// The current lifecycle status of a monitored DCC process.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ProcessStatus {
    /// Process is running and responsive.
    Running,
    /// Process has been spawned but we have not yet confirmed it is ready.
    Starting,
    /// Process has exited cleanly.
    Stopped,
    /// Process crashed (non-zero exit or signal termination).
    Crashed,
    /// Process is unresponsive (heartbeat / main-thread check failed).
    Unresponsive,
    /// Process is being restarted by the crash-recovery policy.
    Restarting,
}

impl std::fmt::Display for ProcessStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Running => write!(f, "running"),
            Self::Starting => write!(f, "starting"),
            Self::Stopped => write!(f, "stopped"),
            Self::Crashed => write!(f, "crashed"),
            Self::Unresponsive => write!(f, "unresponsive"),
            Self::Restarting => write!(f, "restarting"),
        }
    }
}

/// A snapshot of a DCC process's resource usage and lifecycle state.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ProcessInfo {
    /// OS-assigned process identifier.
    pub pid: u32,
    /// Human-readable name (e.g. `"maya"`, `"blender"`).
    pub name: String,
    /// Current lifecycle status.
    pub status: ProcessStatus,
    /// CPU usage as a percentage (0.0–100.0).
    pub cpu_usage_percent: f32,
    /// Resident memory usage in bytes.
    pub memory_bytes: u64,
    /// Number of times this process has been restarted by the recovery policy.
    pub restart_count: u32,
}

impl ProcessInfo {
    /// Create a minimal `ProcessInfo` with zero resource counters.
    pub fn new(pid: u32, name: impl Into<String>, status: ProcessStatus) -> Self {
        Self {
            pid,
            name: name.into(),
            status,
            cpu_usage_percent: 0.0,
            memory_bytes: 0,
            restart_count: 0,
        }
    }

    /// Returns `true` if the process is in a healthy, running state.
    #[must_use]
    pub fn is_healthy(&self) -> bool {
        self.status == ProcessStatus::Running
    }

    /// Returns `true` if the process needs attention (crashed or unresponsive).
    #[must_use]
    pub fn needs_recovery(&self) -> bool {
        matches!(
            self.status,
            ProcessStatus::Crashed | ProcessStatus::Unresponsive
        )
    }
}

/// Configuration for launching and monitoring a specific DCC application.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct DccProcessConfig {
    /// Logical name for this DCC entry (e.g. `"maya-2025"`).
    pub name: String,
    /// Full path to the DCC executable.
    pub executable: String,
    /// Optional command-line arguments passed at launch.
    pub args: Vec<String>,
    /// Maximum number of automatic restart attempts before giving up.
    pub max_restarts: u32,
    /// Milliseconds to wait for the process to become ready after launch.
    pub launch_timeout_ms: u64,
    /// Milliseconds between health-check polls.
    pub poll_interval_ms: u64,
}

impl DccProcessConfig {
    /// Create a config with sensible defaults.
    pub fn new(name: impl Into<String>, executable: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            executable: executable.into(),
            args: Vec::new(),
            max_restarts: 3,
            launch_timeout_ms: 30_000,
            poll_interval_ms: 2_000,
        }
    }

    /// Builder-style method to set command-line arguments.
    #[must_use]
    pub fn with_args(mut self, args: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.args = args.into_iter().map(Into::into).collect();
        self
    }

    /// Builder-style method to set the max restart count.
    #[must_use]
    pub fn with_max_restarts(mut self, n: u32) -> Self {
        self.max_restarts = n;
        self
    }

    /// Builder-style method to set the launch timeout.
    #[must_use]
    pub fn with_launch_timeout_ms(mut self, ms: u64) -> Self {
        self.launch_timeout_ms = ms;
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    mod test_process_status {
        use super::*;

        #[test]
        fn display_variants() {
            assert_eq!(ProcessStatus::Running.to_string(), "running");
            assert_eq!(ProcessStatus::Starting.to_string(), "starting");
            assert_eq!(ProcessStatus::Stopped.to_string(), "stopped");
            assert_eq!(ProcessStatus::Crashed.to_string(), "crashed");
            assert_eq!(ProcessStatus::Unresponsive.to_string(), "unresponsive");
            assert_eq!(ProcessStatus::Restarting.to_string(), "restarting");
        }

        #[test]
        fn equality() {
            assert_eq!(ProcessStatus::Running, ProcessStatus::Running);
            assert_ne!(ProcessStatus::Running, ProcessStatus::Stopped);
        }

        #[test]
        fn serialize_roundtrip() {
            let status = ProcessStatus::Crashed;
            let json = serde_json::to_string(&status).unwrap();
            assert_eq!(json, "\"crashed\"");
            let back: ProcessStatus = serde_json::from_str(&json).unwrap();
            assert_eq!(back, status);
        }
    }

    mod test_process_info {
        use super::*;

        #[test]
        fn new_sets_defaults() {
            let info = ProcessInfo::new(1234, "maya", ProcessStatus::Running);
            assert_eq!(info.pid, 1234);
            assert_eq!(info.name, "maya");
            assert_eq!(info.status, ProcessStatus::Running);
            assert_eq!(info.cpu_usage_percent, 0.0);
            assert_eq!(info.memory_bytes, 0);
            assert_eq!(info.restart_count, 0);
        }

        #[test]
        fn is_healthy_only_when_running() {
            let running = ProcessInfo::new(1, "maya", ProcessStatus::Running);
            let crashed = ProcessInfo::new(2, "maya", ProcessStatus::Crashed);
            assert!(running.is_healthy());
            assert!(!crashed.is_healthy());
        }

        #[test]
        fn needs_recovery_for_crashed_and_unresponsive() {
            let crashed = ProcessInfo::new(1, "maya", ProcessStatus::Crashed);
            let unresponsive = ProcessInfo::new(2, "maya", ProcessStatus::Unresponsive);
            let running = ProcessInfo::new(3, "maya", ProcessStatus::Running);
            assert!(crashed.needs_recovery());
            assert!(unresponsive.needs_recovery());
            assert!(!running.needs_recovery());
        }

        #[test]
        fn serialize_roundtrip() {
            let info = ProcessInfo {
                pid: 42,
                name: "blender".to_string(),
                status: ProcessStatus::Starting,
                cpu_usage_percent: 12.5,
                memory_bytes: 1024 * 1024 * 512,
                restart_count: 1,
            };
            let json = serde_json::to_string(&info).unwrap();
            let back: ProcessInfo = serde_json::from_str(&json).unwrap();
            assert_eq!(back.pid, 42);
            assert_eq!(back.name, "blender");
            assert_eq!(back.status, ProcessStatus::Starting);
            assert!((back.cpu_usage_percent - 12.5).abs() < f32::EPSILON);
        }
    }

    mod test_dcc_process_config {
        use super::*;

        #[test]
        fn new_has_sensible_defaults() {
            let cfg = DccProcessConfig::new("maya-2025", "/usr/bin/maya");
            assert_eq!(cfg.name, "maya-2025");
            assert_eq!(cfg.executable, "/usr/bin/maya");
            assert!(cfg.args.is_empty());
            assert_eq!(cfg.max_restarts, 3);
            assert_eq!(cfg.launch_timeout_ms, 30_000);
            assert_eq!(cfg.poll_interval_ms, 2_000);
        }

        #[test]
        fn builder_with_args() {
            let cfg =
                DccProcessConfig::new("blender", "blender").with_args(["-b", "-P", "script.py"]);
            assert_eq!(cfg.args, vec!["-b", "-P", "script.py"]);
        }

        #[test]
        fn builder_with_max_restarts() {
            let cfg = DccProcessConfig::new("maya", "maya").with_max_restarts(5);
            assert_eq!(cfg.max_restarts, 5);
        }

        #[test]
        fn builder_with_launch_timeout() {
            let cfg = DccProcessConfig::new("houdini", "houdini").with_launch_timeout_ms(60_000);
            assert_eq!(cfg.launch_timeout_ms, 60_000);
        }

        #[test]
        fn serialize_roundtrip() {
            let cfg = DccProcessConfig::new("ue5", "UnrealEditor.exe");
            let json = serde_json::to_string(&cfg).unwrap();
            let back: DccProcessConfig = serde_json::from_str(&json).unwrap();
            assert_eq!(back.name, "ue5");
            assert_eq!(back.executable, "UnrealEditor.exe");
        }
    }
}
