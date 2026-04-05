//! Error types for the dcc-mcp-process crate.

use thiserror::Error;

/// Errors that can occur during DCC process operations.
#[derive(Debug, Error)]
pub enum ProcessError {
    /// The specified process was not found (by PID or name).
    #[error("process not found: {pid}")]
    NotFound { pid: u32 },

    /// Failed to spawn a new process.
    #[error("failed to spawn process '{command}': {reason}")]
    SpawnFailed { command: String, reason: String },

    /// Failed to terminate a process.
    #[error("failed to terminate process {pid}: {reason}")]
    TerminateFailed { pid: u32, reason: String },

    /// Process launch timed out waiting for readiness.
    #[error("process launch timed out after {timeout_ms}ms (command: '{command}')")]
    LaunchTimeout { command: String, timeout_ms: u64 },

    /// The crash recovery loop hit the maximum restart limit.
    #[error("process '{name}' exceeded max restarts ({max_restarts})")]
    MaxRestartsExceeded { name: String, max_restarts: u32 },

    /// An I/O error occurred while interacting with the process.
    #[error("I/O error for process {pid}: {reason}")]
    Io { pid: u32, reason: String },

    /// The monitor was already shut down when an operation was attempted.
    #[error("process monitor is shut down")]
    MonitorShutdown,

    /// A generic internal error.
    #[error("internal error: {0}")]
    Internal(String),
}

impl ProcessError {
    /// Convenience constructor for `SpawnFailed`.
    pub fn spawn_failed(command: impl Into<String>, reason: impl Into<String>) -> Self {
        Self::SpawnFailed {
            command: command.into(),
            reason: reason.into(),
        }
    }

    /// Convenience constructor for `Internal`.
    pub fn internal(msg: impl Into<String>) -> Self {
        Self::Internal(msg.into())
    }
}

// ── Tests ──────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    mod test_display {
        use super::*;

        #[test]
        fn not_found_display() {
            let err = ProcessError::NotFound { pid: 42 };
            let s = err.to_string();
            assert!(s.contains("42"), "{s}");
        }

        #[test]
        fn spawn_failed_display() {
            let err = ProcessError::SpawnFailed {
                command: "maya".to_string(),
                reason: "not found".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("maya"), "{s}");
            assert!(s.contains("not found"), "{s}");
        }

        #[test]
        fn terminate_failed_display() {
            let err = ProcessError::TerminateFailed {
                pid: 1234,
                reason: "access denied".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("1234"), "{s}");
            assert!(s.contains("access denied"), "{s}");
        }

        #[test]
        fn launch_timeout_display() {
            let err = ProcessError::LaunchTimeout {
                command: "houdini".to_string(),
                timeout_ms: 30_000,
            };
            let s = err.to_string();
            assert!(s.contains("houdini"), "{s}");
            assert!(s.contains("30000"), "{s}");
        }

        #[test]
        fn max_restarts_exceeded_display() {
            let err = ProcessError::MaxRestartsExceeded {
                name: "maya_worker".to_string(),
                max_restarts: 5,
            };
            let s = err.to_string();
            assert!(s.contains("maya_worker"), "{s}");
            assert!(s.contains('5'), "{s}");
        }

        #[test]
        fn io_display() {
            let err = ProcessError::Io {
                pid: 99,
                reason: "pipe broken".to_string(),
            };
            let s = err.to_string();
            assert!(s.contains("99"), "{s}");
            assert!(s.contains("pipe broken"), "{s}");
        }

        #[test]
        fn monitor_shutdown_display() {
            let err = ProcessError::MonitorShutdown;
            let s = err.to_string();
            assert!(s.contains("shut down"), "{s}");
        }

        #[test]
        fn internal_display() {
            let err = ProcessError::Internal("unexpected state".to_string());
            let s = err.to_string();
            assert!(s.contains("unexpected state"), "{s}");
        }
    }

    mod test_constructors {
        use super::*;

        #[test]
        fn spawn_failed_constructor() {
            let err = ProcessError::spawn_failed("blender", "not in PATH");
            match err {
                ProcessError::SpawnFailed { command, reason } => {
                    assert_eq!(command, "blender");
                    assert_eq!(reason, "not in PATH");
                }
                other => panic!("unexpected: {other:?}"),
            }
        }

        #[test]
        fn internal_constructor() {
            let err = ProcessError::internal("bad state");
            assert!(matches!(err, ProcessError::Internal(s) if s == "bad state"));
        }
    }

    mod test_debug {
        use super::*;

        #[test]
        fn all_variants_are_debug() {
            let variants = vec![
                ProcessError::NotFound { pid: 1 },
                ProcessError::SpawnFailed {
                    command: "c".to_string(),
                    reason: "r".to_string(),
                },
                ProcessError::TerminateFailed {
                    pid: 2,
                    reason: "r".to_string(),
                },
                ProcessError::LaunchTimeout {
                    command: "c".to_string(),
                    timeout_ms: 1,
                },
                ProcessError::MaxRestartsExceeded {
                    name: "n".to_string(),
                    max_restarts: 3,
                },
                ProcessError::Io {
                    pid: 4,
                    reason: "r".to_string(),
                },
                ProcessError::MonitorShutdown,
                ProcessError::Internal("i".to_string()),
            ];
            for v in &variants {
                assert!(!format!("{v:?}").is_empty());
            }
        }
    }
}
