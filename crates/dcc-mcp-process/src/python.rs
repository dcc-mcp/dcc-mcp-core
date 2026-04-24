//! PyO3 bindings for the `dcc-mcp-process` crate.
//!
//! Exposes `ProcessMonitor`, `DccLauncher`, `CrashRecoveryPolicy`,
//! `ProcessWatcher`, and the two dispatcher flavours to Python as:
//!
//! ```text
//! from dcc_mcp_core import (
//!     PyProcessMonitor,
//!     PyDccLauncher,
//!     PyCrashRecoveryPolicy,
//!     PyProcessWatcher,
//!     PyStandaloneDispatcher,
//!     PyPumpedDispatcher,
//! )
//! ```
//!
//! ## Maintainer layout
//!
//! `python.rs` is a thin facade; every `Py*` class lives in a focused sibling:
//!
//! - [`helpers`] — shared Tokio runtime, `ProcessError → PyErr`, status→str
//! - [`monitor`] — `PyProcessMonitor`
//! - [`launcher`] — `PyDccLauncher`
//! - [`crash_policy`] — `PyCrashRecoveryPolicy` (+ `parse_status`)
//! - [`watcher`] — `PyProcessWatcher` (+ internal `PyWatcherEvent`)
//! - [`standalone_dispatcher`] — `PyStandaloneDispatcher`
//! - [`pumped_dispatcher`] — `PyPumpedDispatcher` (+ `parse_affinity`,
//!   `outcome_to_dict`)

use pyo3::prelude::*;

#[path = "python_helpers.rs"]
mod helpers;

#[path = "python_monitor.rs"]
mod monitor;

#[path = "python_launcher.rs"]
mod launcher;

#[path = "python_crash_policy.rs"]
mod crash_policy;

#[path = "python_watcher.rs"]
mod watcher;

#[path = "python_standalone_dispatcher.rs"]
mod standalone_dispatcher;

#[path = "python_pumped_dispatcher.rs"]
mod pumped_dispatcher;

pub use crash_policy::PyCrashRecoveryPolicy;
pub use launcher::PyDccLauncher;
pub use monitor::PyProcessMonitor;
pub use pumped_dispatcher::PyPumpedDispatcher;
pub use standalone_dispatcher::PyStandaloneDispatcher;
pub use watcher::PyProcessWatcher;

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
