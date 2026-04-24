//! Rolling-file logging layer for the global `tracing` subscriber.
//!
//! The writer rotates on **either** a configured byte size **or** a
//! calendar-date change (local time). It plugs into the subscriber
//! that [`crate::log_config::init_logging`] installs via a reload
//! handle, so callers can opt in at any time — including from Python
//! (`init_file_logging`) after the `_core` module has already loaded.
//!
//! ## Design
//!
//! ```text
//! tracing events
//!     │
//!     ▼
//! fmt::Layer<Registry, non_blocking_writer>
//!     │
//!     ▼ (channel, lossy = false)
//! tracing_appender::non_blocking worker thread
//!     │
//!     ▼
//! RollingFileWriter (Mutex<Inner>):
//!     - open current file (<prefix>.<YYYYMMDD>.log)
//!     - check size + date on each write
//!     - rotate → <prefix>.<YYYYMMDDTHHMMSS>.log, prune oldest
//! ```
//!
//! Thread-safe via the inner `parking_lot::Mutex`. The non-blocking
//! worker serializes writes from all call sites, but we still guard
//! rotation state so other direct writers (tests) stay sound.
//!
//! The `tracing_appender::non_blocking` worker returns a
//! `WorkerGuard` that **must** outlive the process — we park it in a
//! `OnceLock` alongside the optional midnight-ticker handle.
//!
//! ## Maintainer layout
//!
//! This module is a **thin facade**. Responsibilities are divided
//! across sibling files, each under ~300 lines:
//!
//! | File | Responsibility |
//! |------|----------------|
//! | `file_logging_config.rs` | `RotationPolicy`, `FileLoggingConfig`, `FileLoggingError`, env-var parsing |
//! | `file_logging_writer.rs` | `RollingFileWriter`, `Inner`, `CalendarDate`, filesystem helpers |
//! | `file_logging_python.rs` | `#[pyclass] PyFileLoggingConfig` + `py_*` entry points |
//! | `file_logging_tests.rs`  | Unit tests (rotation, retention, install/shutdown idempotency) |
//!
//! This file keeps the public install / shutdown / flush entry points
//! and the process-wide handles (`FileLoggingHandles`).

#[path = "file_logging_config.rs"]
mod config;

#[path = "file_logging_writer.rs"]
mod writer;

#[cfg(feature = "python-bindings")]
#[path = "file_logging_python.rs"]
pub mod python;

#[cfg(test)]
#[path = "file_logging_tests.rs"]
mod tests;

pub use config::{FileLoggingConfig, FileLoggingError, RotationPolicy};
pub use writer::RollingFileWriter;

use crate::log_config::{BoxedLayer, install_file_layer_boxed};

use parking_lot::Mutex;
use std::path::PathBuf;
use std::sync::Arc;
use std::sync::OnceLock;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

// ── Layer installation ───────────────────────────────────────────────────────

/// Process-wide handles kept alive for the lifetime of file logging.
///
/// `WorkerGuard` must outlive the subscriber for the async worker to
/// flush pending buffers on shutdown.
#[allow(dead_code)] // fields are kept alive via Drop semantics
struct FileLoggingHandles {
    guard: WorkerGuard,
    config: FileLoggingConfig,
    /// Shared `Inner` from the live `RollingFileWriter`. Cloning the writer
    /// is cheap (it only clones the `Arc`) so we keep a copy here to service
    /// `flush_logs()` calls from Python / Rust without going through the
    /// async `tracing_appender` channel (issue #402).
    writer_inner: Arc<Mutex<writer::Inner>>,
}

static HANDLES: OnceLock<parking_lot::Mutex<Option<FileLoggingHandles>>> = OnceLock::new();

fn handles_slot() -> &'static parking_lot::Mutex<Option<FileLoggingHandles>> {
    HANDLES.get_or_init(|| parking_lot::Mutex::new(None))
}

/// Install (or replace) the rolling-file layer on the global subscriber.
///
/// Calls [`crate::log_config::init_logging`] first to make sure the
/// reload handle exists. Subsequent calls swap the layer atomically —
/// useful for tests that point at different directories.
///
/// # Errors
/// - [`FileLoggingError::Config`] for malformed env vars.
/// - [`FileLoggingError::Io`] if the log directory / file cannot be opened.
/// - [`FileLoggingError::Install`] if the reload handle refuses the swap.
pub fn init_file_logging(config: FileLoggingConfig) -> Result<PathBuf, FileLoggingError> {
    // Ensure the subscriber (and its reload handle) exist.
    crate::log_config::init_logging();

    let writer = RollingFileWriter::new(&config)?;
    let writer_inner = writer.inner.clone();
    let directory = writer_inner.lock().directory.clone();

    let (non_blocking, guard): (NonBlocking, WorkerGuard) = tracing_appender::non_blocking(writer);

    let fmt_layer = tracing_subscriber::fmt::layer()
        .with_target(true)
        .with_thread_names(true)
        .with_ansi(false)
        .with_writer(non_blocking);

    let boxed: BoxedLayer<tracing_subscriber::Registry> = Box::new(fmt_layer);
    install_file_layer_boxed(Some(boxed))?;

    // Drop any previous guard AFTER installing the new layer so buffered
    // events from the old writer flush cleanly.
    *handles_slot().lock() = Some(FileLoggingHandles {
        guard,
        config: config.clone(),
        writer_inner,
    });

    Ok(directory)
}

/// Flush any buffered log events to disk immediately.
///
/// `tracing_appender::non_blocking` batches writes on a background
/// thread and only guarantees a flush on rotation or `WorkerGuard` drop.
/// For long-running DCC sessions (Maya, Blender…) this means the log
/// file can appear empty or stale until the process exits.
///
/// Calling `flush_logs()` forces the underlying `RollingFileWriter` to
/// flush its OS page-cache buffers, making all events written so far
/// visible on disk immediately. Issue #402.
///
/// Safe to call from any thread. Returns `Ok(())` when no file layer is
/// installed (no-op).
pub fn flush_logs() -> std::io::Result<()> {
    use std::io::Write;
    if let Some(handles) = handles_slot().lock().as_ref() {
        handles.writer_inner.lock().current.flush()?;
    }
    Ok(())
}

/// Uninstall the rolling-file layer (console output is unaffected).
///
/// Safe to call when no file layer is currently installed — it is a
/// no-op in that case.
///
/// # Errors
/// Returns [`FileLoggingError::Install`] if the reload mechanism is not
/// initialized (logging never set up).
pub fn shutdown_file_logging() -> Result<(), FileLoggingError> {
    install_file_layer_boxed(None)?;
    *handles_slot().lock() = None;
    Ok(())
}
