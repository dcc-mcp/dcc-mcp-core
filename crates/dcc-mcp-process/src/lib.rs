//! # dcc-mcp-process
//!
//! Cross-platform DCC process monitoring, lifecycle management, and crash
//! recovery for the DCC-MCP ecosystem.
//!
//! ## Modules
//!
//! | Module | Purpose |
//! |--------|---------|
//! | [`types`] | Core data types: `ProcessInfo`, `ProcessStatus`, `DccProcessConfig` |
//! | [`error`] | `ProcessError` enum |
//! | [`monitor`] | `ProcessMonitor` — live resource snapshots via `sysinfo` |
//! | [`launcher`] | `DccLauncher` — async spawn/terminate/kill |
//! | [`recovery`] | `CrashRecoveryPolicy` — restart decision engine |
//! | [`watcher`] | `ProcessWatcher` — async background watch loop with event channel |
//!
//! ## Example
//!
//! ```text
//! use dcc_mcp_process::{
//!     monitor::ProcessMonitor,
//!     types::{DccProcessConfig, ProcessStatus},
//!     recovery::CrashRecoveryPolicy,
//! };
//!
//! // Monitor the current process
//! let mut monitor = ProcessMonitor::new();
//! monitor.track(std::process::id(), "self");
//! monitor.refresh();
//! if let Some(info) = monitor.query(std::process::id()) {
//!     println!("cpu={:.1}% mem={}B", info.cpu_usage_percent, info.memory_bytes);
//! }
//!
//! // Configure crash recovery
//! let policy = CrashRecoveryPolicy::new(3)
//!     .with_exponential_backoff(1_000, 30_000);
//! assert!(policy.should_restart(ProcessStatus::Crashed));
//! ```

pub mod error;
pub mod launcher;
pub mod monitor;
pub mod recovery;
pub mod types;
pub mod watcher;

#[cfg(feature = "python-bindings")]
pub mod python;

// Convenient re-exports at the crate root
pub use error::ProcessError;
pub use launcher::DccLauncher;
pub use monitor::ProcessMonitor;
pub use recovery::{BackoffStrategy, CrashRecoveryPolicy};
pub use types::{DccProcessConfig, ProcessInfo, ProcessStatus};
pub use watcher::{ProcessEvent, ProcessWatcher, WatcherHandle};

#[cfg(feature = "python-bindings")]
pub use python::{PyCrashRecoveryPolicy, PyDccLauncher, PyProcessMonitor, PyProcessWatcher};
