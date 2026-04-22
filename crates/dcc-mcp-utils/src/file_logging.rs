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

use crate::constants::{
    DEFAULT_LOG_FILE_PREFIX, DEFAULT_LOG_MAX_FILES, DEFAULT_LOG_MAX_SIZE, DEFAULT_LOG_ROTATION,
    ENV_LOG_DIR, ENV_LOG_FILE, ENV_LOG_FILE_PREFIX, ENV_LOG_MAX_FILES, ENV_LOG_MAX_SIZE,
    ENV_LOG_ROTATION,
};
use crate::filesystem::get_log_dir;
use crate::log_config::{BoxedLayer, FileLayerInstallError, install_file_layer_boxed};

use parking_lot::Mutex;
use std::fs::{File, OpenOptions};
use std::io::{self, Write};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::OnceLock;
use time::OffsetDateTime;
use time::macros::format_description;
use tracing_appender::non_blocking::{NonBlocking, WorkerGuard};

// ── Configuration ────────────────────────────────────────────────────────────

/// Rotation policy used by [`RollingFileWriter`].
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum RotationPolicy {
    /// Rotate only when the configured size threshold is surpassed.
    Size,
    /// Rotate only when the calendar date (local time) changes.
    Daily,
    /// Rotate on size **or** date — whichever fires first.
    Both,
}

impl RotationPolicy {
    /// Parse a case-insensitive string (`"size"` / `"daily"` / `"both"`).
    ///
    /// # Errors
    /// Returns an error string for unknown policies.
    pub fn parse(s: &str) -> Result<Self, String> {
        match s.trim().to_ascii_lowercase().as_str() {
            "size" => Ok(Self::Size),
            "daily" | "date" => Ok(Self::Daily),
            "both" | "size+daily" | "size+date" => Ok(Self::Both),
            other => Err(format!(
                "unknown rotation policy '{other}' (expected: size|daily|both)"
            )),
        }
    }

    fn rotates_on_size(self) -> bool {
        matches!(self, Self::Size | Self::Both)
    }

    fn rotates_on_date(self) -> bool {
        matches!(self, Self::Daily | Self::Both)
    }

    /// Stable lower-case string representation — used by the Python getter
    /// and for reconstructing configs from env vars.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Size => "size",
            Self::Daily => "daily",
            Self::Both => "both",
        }
    }
}

/// Configuration for the rolling-file logger.
///
/// Every knob has a sensible default; all can be overridden by the
/// matching `DCC_MCP_LOG_*` environment variable when building via
/// [`FileLoggingConfig::from_env_with_defaults`].
#[derive(Debug, Clone)]
pub struct FileLoggingConfig {
    /// Directory to write logs into. `None` = use [`get_log_dir`].
    pub directory: Option<PathBuf>,
    /// File-name stem (the full file is `<prefix>.<date>.log`).
    pub file_name_prefix: String,
    /// Maximum bytes per file before size-triggered rotation.
    pub max_size_bytes: u64,
    /// Number of **rolled** files to retain (current file excluded).
    pub max_files: usize,
    /// Rotation policy.
    pub rotation: RotationPolicy,
    /// Keep the console `fmt::Layer` active in parallel with the file
    /// layer. Informational for now — the console layer is managed by
    /// [`crate::log_config::init_logging`] and is always installed; this
    /// flag is surfaced for future parity with Python where a user may
    /// wish to silence stderr when redirecting to a file.
    pub include_console: bool,
}

impl Default for FileLoggingConfig {
    fn default() -> Self {
        Self {
            directory: None,
            file_name_prefix: DEFAULT_LOG_FILE_PREFIX.to_string(),
            max_size_bytes: DEFAULT_LOG_MAX_SIZE,
            max_files: DEFAULT_LOG_MAX_FILES,
            rotation: RotationPolicy::parse(DEFAULT_LOG_ROTATION).unwrap_or(RotationPolicy::Both),
            include_console: true,
        }
    }
}

impl FileLoggingConfig {
    /// Build a config, overlaying any `DCC_MCP_LOG_*` env vars on top of the defaults.
    ///
    /// Env vars:
    /// - [`ENV_LOG_DIR`] — directory path.
    /// - [`ENV_LOG_FILE_PREFIX`] — file-name prefix.
    /// - [`ENV_LOG_MAX_SIZE`] — bytes (integer).
    /// - [`ENV_LOG_MAX_FILES`] — retention count (integer).
    /// - [`ENV_LOG_ROTATION`] — `size`/`daily`/`both`.
    ///
    /// # Errors
    /// Returns an error if any env var is set to an invalid value.
    pub fn from_env_with_defaults() -> Result<Self, FileLoggingError> {
        let mut cfg = Self::default();

        if let Ok(dir) = std::env::var(ENV_LOG_DIR) {
            if !dir.trim().is_empty() {
                cfg.directory = Some(PathBuf::from(dir));
            }
        }
        if let Ok(prefix) = std::env::var(ENV_LOG_FILE_PREFIX) {
            if !prefix.trim().is_empty() {
                cfg.file_name_prefix = prefix;
            }
        }
        if let Ok(raw) = std::env::var(ENV_LOG_MAX_SIZE) {
            cfg.max_size_bytes = raw.parse().map_err(|_| {
                FileLoggingError::Config(format!("{ENV_LOG_MAX_SIZE}='{raw}' is not a valid u64"))
            })?;
        }
        if let Ok(raw) = std::env::var(ENV_LOG_MAX_FILES) {
            cfg.max_files = raw.parse().map_err(|_| {
                FileLoggingError::Config(format!(
                    "{ENV_LOG_MAX_FILES}='{raw}' is not a valid usize"
                ))
            })?;
        }
        if let Ok(raw) = std::env::var(ENV_LOG_ROTATION) {
            cfg.rotation = RotationPolicy::parse(&raw).map_err(FileLoggingError::Config)?;
        }

        Ok(cfg)
    }

    /// Returns `true` if the [`ENV_LOG_FILE`] env var is set to a truthy value.
    ///
    /// Accepts `1`, `true`, `yes`, `on` (case-insensitive). Unset or any
    /// other value returns `false`.
    #[must_use]
    pub fn enabled_by_env() -> bool {
        match std::env::var(ENV_LOG_FILE) {
            Ok(v) => matches!(
                v.trim().to_ascii_lowercase().as_str(),
                "1" | "true" | "yes" | "on"
            ),
            Err(_) => false,
        }
    }

    fn resolved_directory(&self) -> Result<PathBuf, FileLoggingError> {
        if let Some(dir) = &self.directory {
            std::fs::create_dir_all(dir).map_err(FileLoggingError::Io)?;
            Ok(dir.clone())
        } else {
            let dir = get_log_dir().map_err(|e| FileLoggingError::Config(e.to_string()))?;
            Ok(PathBuf::from(dir))
        }
    }
}

// ── Errors ───────────────────────────────────────────────────────────────────

/// Errors surfaced when installing or configuring file logging.
#[derive(Debug)]
#[non_exhaustive]
pub enum FileLoggingError {
    /// Invalid config value (bad env var, unknown rotation policy, etc.).
    Config(String),
    /// Underlying I/O failure while creating the directory or log file.
    Io(io::Error),
    /// The reload mechanism in [`crate::log_config`] is not yet initialized
    /// or refused the swap.
    Install(FileLayerInstallError),
}

impl std::fmt::Display for FileLoggingError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Config(msg) => write!(f, "file-logging config error: {msg}"),
            Self::Io(e) => write!(f, "file-logging I/O error: {e}"),
            Self::Install(e) => write!(f, "file-logging install error: {e}"),
        }
    }
}

impl std::error::Error for FileLoggingError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            Self::Install(e) => Some(e),
            _ => None,
        }
    }
}

impl From<FileLayerInstallError> for FileLoggingError {
    fn from(err: FileLayerInstallError) -> Self {
        Self::Install(err)
    }
}

#[cfg(feature = "python-bindings")]
impl From<FileLoggingError> for pyo3::PyErr {
    fn from(err: FileLoggingError) -> pyo3::PyErr {
        match err {
            FileLoggingError::Config(_) => pyo3::exceptions::PyValueError::new_err(err.to_string()),
            FileLoggingError::Io(_) => pyo3::exceptions::PyOSError::new_err(err.to_string()),
            FileLoggingError::Install(_) => {
                pyo3::exceptions::PyRuntimeError::new_err(err.to_string())
            }
        }
    }
}

// ── Rolling writer ───────────────────────────────────────────────────────────

/// Thread-safe rolling writer — size **and/or** calendar-date triggered.
///
/// Consumed by [`tracing_appender::non_blocking`] to get async flushing.
#[derive(Debug)]
pub struct RollingFileWriter {
    inner: Arc<Mutex<Inner>>,
}

#[derive(Debug)]
struct Inner {
    directory: PathBuf,
    prefix: String,
    max_size: u64,
    max_files: usize,
    rotation: RotationPolicy,
    current: File,
    current_size: u64,
    current_date: CalendarDate,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct CalendarDate {
    year: i32,
    month: u8,
    day: u8,
}

impl CalendarDate {
    fn today_local() -> Self {
        let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
        Self {
            year: now.year(),
            month: now.month() as u8,
            day: now.day(),
        }
    }

    fn as_basename(self) -> String {
        format!("{:04}{:02}{:02}", self.year, self.month, self.day)
    }
}

impl RollingFileWriter {
    /// Build a writer from a resolved configuration.
    ///
    /// # Errors
    /// Fails if the log directory cannot be created or the initial log
    /// file cannot be opened for append.
    pub fn new(config: &FileLoggingConfig) -> Result<Self, FileLoggingError> {
        let directory = config.resolved_directory()?;
        let current_date = CalendarDate::today_local();
        let current_path = current_path(&directory, &config.file_name_prefix, current_date);
        let current = open_append(&current_path).map_err(FileLoggingError::Io)?;
        let current_size = current
            .metadata()
            .map(|m| m.len())
            .map_err(FileLoggingError::Io)?;

        Ok(Self {
            inner: Arc::new(Mutex::new(Inner {
                directory,
                prefix: config.file_name_prefix.clone(),
                max_size: config.max_size_bytes.max(1),
                max_files: config.max_files,
                rotation: config.rotation,
                current,
                current_size,
                current_date,
            })),
        })
    }
}

impl Inner {
    /// Check whether the current file needs rotating *before* a write
    /// of `incoming` bytes. Rotates if needed.
    fn maybe_rotate(&mut self, incoming: usize) -> io::Result<()> {
        let today = CalendarDate::today_local();

        let size_trigger = self.rotation.rotates_on_size()
            && self.current_size.saturating_add(incoming as u64) > self.max_size
            && self.current_size > 0;
        let date_trigger = self.rotation.rotates_on_date() && today != self.current_date;

        if !size_trigger && !date_trigger {
            return Ok(());
        }

        // Best-effort flush before rename.
        let _ = self.current.flush();

        // Drop the old handle by replacing with a dummy, so the file
        // is closed on Windows (which forbids renaming an open file).
        let placeholder = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(self.directory.join(".dcc-mcp-rotate-tmp"))?;
        let old = std::mem::replace(&mut self.current, placeholder);
        drop(old);

        let old_current = current_path(&self.directory, &self.prefix, self.current_date);
        if old_current.exists() {
            let rotated = rotated_path(&self.directory, &self.prefix);
            // Ignore rotate-rename failures silently — we'd rather keep
            // logging to the current file than panic on EBUSY.
            let _ = std::fs::rename(&old_current, &rotated);
        }

        // Update date and open the new current file.
        self.current_date = today;
        let new_current = current_path(&self.directory, &self.prefix, self.current_date);
        self.current = open_append(&new_current)?;
        self.current_size = self.current.metadata().map(|m| m.len()).unwrap_or_default();

        // Clean the placeholder file after successful rotation.
        let _ = std::fs::remove_file(self.directory.join(".dcc-mcp-rotate-tmp"));

        // Retention pruning.
        prune_old(&self.directory, &self.prefix, self.max_files);

        Ok(())
    }
}

impl Write for RollingFileWriter {
    fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
        let mut inner = self.inner.lock();
        inner.maybe_rotate(buf.len())?;
        let n = inner.current.write(buf)?;
        inner.current_size = inner.current_size.saturating_add(n as u64);
        Ok(n)
    }

    fn flush(&mut self) -> io::Result<()> {
        self.inner.lock().current.flush()
    }
}

// ── File helpers ─────────────────────────────────────────────────────────────

fn open_append(path: &Path) -> io::Result<File> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    OpenOptions::new()
        .create(true)
        .append(true)
        .read(false)
        .open(path)
}

/// `<directory>/<prefix>.<YYYYMMDD>.log`
fn current_path(directory: &Path, prefix: &str, date: CalendarDate) -> PathBuf {
    directory.join(format!("{prefix}.{}.log", date.as_basename()))
}

/// Filename used when rolling out an existing file — includes time-of-day
/// so size-triggered rotations within the same day remain sortable.
fn rotated_path(directory: &Path, prefix: &str) -> PathBuf {
    let now = OffsetDateTime::now_local().unwrap_or_else(|_| OffsetDateTime::now_utc());
    let fmt = format_description!("[year][month][day]T[hour][minute][second]");
    let stamp = now.format(&fmt).unwrap_or_else(|_| {
        format!(
            "{:04}{:02}{:02}T{:02}{:02}{:02}",
            now.year(),
            now.month() as u8,
            now.day(),
            now.hour(),
            now.minute(),
            now.second(),
        )
    });

    // Collision-safe: append a `.N` counter if the timestamp already exists
    // (possible on rapid successive rotations at sub-second resolution).
    let base = directory.join(format!("{prefix}.{stamp}.log"));
    if !base.exists() {
        return base;
    }
    for n in 1..1000 {
        let candidate = directory.join(format!("{prefix}.{stamp}.{n}.log"));
        if !candidate.exists() {
            return candidate;
        }
    }
    base
}

/// Keep the most recent `max_files` **rolled** files; delete older ones.
///
/// The "current" file (today's `<prefix>.<YYYYMMDD>.log` stem with no
/// hour/minute/second component) is never pruned.
fn prune_old(directory: &Path, prefix: &str, max_files: usize) {
    let Ok(read_dir) = std::fs::read_dir(directory) else {
        return;
    };
    let today_stem = format!("{prefix}.{}", CalendarDate::today_local().as_basename());

    let mut rolled: Vec<(std::time::SystemTime, PathBuf)> = Vec::new();
    for entry in read_dir.flatten() {
        let path = entry.path();
        let Some(name) = path.file_name().and_then(|s| s.to_str()) else {
            continue;
        };
        if !name.starts_with(&format!("{prefix}.")) || !name.ends_with(".log") {
            continue;
        }
        // Skip today's "plain" file (no `T` separator in the timestamp).
        let stem = name.trim_end_matches(".log");
        if stem == today_stem {
            continue;
        }
        let modified = entry
            .metadata()
            .and_then(|m| m.modified())
            .unwrap_or(std::time::UNIX_EPOCH);
        rolled.push((modified, path));
    }

    if rolled.len() <= max_files {
        return;
    }

    // Sort newest → oldest; drop the tail beyond `max_files`.
    rolled.sort_by_key(|entry| std::cmp::Reverse(entry.0));
    for (_, path) in rolled.into_iter().skip(max_files) {
        let _ = std::fs::remove_file(path);
    }
}

// ── Layer installation ───────────────────────────────────────────────────────

/// Process-wide handles kept alive for the lifetime of file logging.
///
/// `WorkerGuard` must outlive the subscriber for the async worker to
/// flush pending buffers on shutdown.
#[allow(dead_code)] // fields are kept alive via Drop semantics
struct FileLoggingHandles {
    guard: WorkerGuard,
    config: FileLoggingConfig,
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
    let directory = writer.inner.lock().directory.clone();

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
    });

    Ok(directory)
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

// ── PyO3 bindings ────────────────────────────────────────────────────────────

#[cfg(feature = "python-bindings")]
pub mod python {
    //! PyO3 wrappers for [`super::FileLoggingConfig`] and [`super::init_file_logging`].

    use super::{
        DEFAULT_LOG_FILE_PREFIX, DEFAULT_LOG_MAX_FILES, DEFAULT_LOG_MAX_SIZE, DEFAULT_LOG_ROTATION,
        FileLoggingConfig, RotationPolicy, init_file_logging, shutdown_file_logging,
    };
    use pyo3::prelude::*;
    use std::path::PathBuf;

    /// Python-facing mirror of `FileLoggingConfig`.
    #[pyclass(
        module = "dcc_mcp_core._core",
        name = "FileLoggingConfig",
        from_py_object
    )]
    #[derive(Debug, Clone)]
    pub struct PyFileLoggingConfig {
        inner: FileLoggingConfig,
    }

    #[pymethods]
    impl PyFileLoggingConfig {
        /// Construct a new config. All kwargs are optional; the defaults
        /// match the `DCC_MCP_LOG_*` env-var fallbacks in Rust.
        #[new]
        #[pyo3(signature = (
            directory = None,
            file_name_prefix = None,
            max_size_bytes = None,
            max_files = None,
            rotation = None,
            include_console = None,
        ))]
        fn new(
            directory: Option<String>,
            file_name_prefix: Option<String>,
            max_size_bytes: Option<u64>,
            max_files: Option<usize>,
            rotation: Option<String>,
            include_console: Option<bool>,
        ) -> PyResult<Self> {
            let mut cfg = FileLoggingConfig::default();
            if let Some(d) = directory {
                if !d.trim().is_empty() {
                    cfg.directory = Some(PathBuf::from(d));
                }
            }
            if let Some(p) = file_name_prefix {
                if !p.trim().is_empty() {
                    cfg.file_name_prefix = p;
                }
            }
            if let Some(s) = max_size_bytes {
                cfg.max_size_bytes = s;
            }
            if let Some(n) = max_files {
                cfg.max_files = n;
            }
            if let Some(r) = rotation {
                cfg.rotation =
                    RotationPolicy::parse(&r).map_err(pyo3::exceptions::PyValueError::new_err)?;
            }
            if let Some(b) = include_console {
                cfg.include_console = b;
            }
            Ok(Self { inner: cfg })
        }

        /// Construct a config pre-populated from `DCC_MCP_LOG_*` env vars.
        #[staticmethod]
        fn from_env() -> PyResult<Self> {
            let cfg = FileLoggingConfig::from_env_with_defaults()?;
            Ok(Self { inner: cfg })
        }

        #[getter]
        fn directory(&self) -> Option<String> {
            self.inner
                .directory
                .as_ref()
                .map(|p| p.to_string_lossy().into_owned())
        }

        #[setter]
        fn set_directory(&mut self, value: Option<String>) {
            self.inner.directory = value.filter(|s| !s.trim().is_empty()).map(PathBuf::from);
        }

        #[getter]
        fn file_name_prefix(&self) -> String {
            self.inner.file_name_prefix.clone()
        }

        #[setter]
        fn set_file_name_prefix(&mut self, value: String) {
            if !value.trim().is_empty() {
                self.inner.file_name_prefix = value;
            }
        }

        #[getter]
        fn max_size_bytes(&self) -> u64 {
            self.inner.max_size_bytes
        }

        #[setter]
        fn set_max_size_bytes(&mut self, value: u64) {
            self.inner.max_size_bytes = value;
        }

        #[getter]
        fn max_files(&self) -> usize {
            self.inner.max_files
        }

        #[setter]
        fn set_max_files(&mut self, value: usize) {
            self.inner.max_files = value;
        }

        #[getter]
        fn rotation(&self) -> String {
            self.inner.rotation.as_str().to_string()
        }

        #[setter]
        fn set_rotation(&mut self, value: String) -> PyResult<()> {
            self.inner.rotation =
                RotationPolicy::parse(&value).map_err(pyo3::exceptions::PyValueError::new_err)?;
            Ok(())
        }

        #[getter]
        fn include_console(&self) -> bool {
            self.inner.include_console
        }

        #[setter]
        fn set_include_console(&mut self, value: bool) {
            self.inner.include_console = value;
        }

        fn __repr__(&self) -> String {
            format!(
                "FileLoggingConfig(directory={:?}, file_name_prefix={:?}, max_size_bytes={}, max_files={}, rotation={:?}, include_console={})",
                self.inner
                    .directory
                    .as_ref()
                    .map(|p| p.to_string_lossy().into_owned()),
                self.inner.file_name_prefix,
                self.inner.max_size_bytes,
                self.inner.max_files,
                self.inner.rotation.as_str(),
                self.inner.include_console,
            )
        }
    }

    impl PyFileLoggingConfig {
        pub(crate) fn into_inner(self) -> FileLoggingConfig {
            self.inner
        }
    }

    /// Install (or replace) file logging. Returns the resolved log directory.
    #[pyfunction]
    #[pyo3(name = "init_file_logging", signature = (config = None))]
    pub fn py_init_file_logging(config: Option<PyFileLoggingConfig>) -> PyResult<String> {
        let cfg = match config {
            Some(c) => c.into_inner(),
            None => FileLoggingConfig::from_env_with_defaults()?,
        };
        let dir = init_file_logging(cfg)?;
        Ok(dir.to_string_lossy().into_owned())
    }

    /// Disable file logging. Console output is unaffected.
    #[pyfunction]
    #[pyo3(name = "shutdown_file_logging")]
    pub fn py_shutdown_file_logging() -> PyResult<()> {
        shutdown_file_logging()?;
        Ok(())
    }

    // Re-export the defaults as Python-visible module constants on request
    // so callers can surface them in UIs without importing from Rust.
    #[pyfunction]
    #[pyo3(name = "_default_file_logging_settings")]
    pub fn py_default_settings() -> (String, u64, usize, String) {
        (
            DEFAULT_LOG_FILE_PREFIX.to_string(),
            DEFAULT_LOG_MAX_SIZE,
            DEFAULT_LOG_MAX_FILES,
            DEFAULT_LOG_ROTATION.to_string(),
        )
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn tmp_dir(tag: &str) -> PathBuf {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "dcc-mcp-file-logging-{tag}-{}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        ));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn parses_rotation_policies() {
        assert_eq!(RotationPolicy::parse("size").unwrap(), RotationPolicy::Size);
        assert_eq!(
            RotationPolicy::parse("DAILY").unwrap(),
            RotationPolicy::Daily
        );
        assert_eq!(RotationPolicy::parse("both").unwrap(), RotationPolicy::Both);
        assert!(RotationPolicy::parse("nonsense").is_err());
    }

    #[test]
    fn size_rotation_creates_rolled_file() {
        let dir = tmp_dir("size");
        let cfg = FileLoggingConfig {
            directory: Some(dir.clone()),
            file_name_prefix: "unit".to_string(),
            max_size_bytes: 32,
            max_files: 3,
            rotation: RotationPolicy::Size,
            include_console: true,
        };
        let mut writer = RollingFileWriter::new(&cfg).unwrap();

        // First write below threshold — no rotation.
        writer.write_all(b"hello\n").unwrap();
        // Second write pushes us past 32 bytes.
        writer.write_all(&[b'x'; 64]).unwrap();
        writer.flush().unwrap();
        drop(writer);

        let entries: Vec<PathBuf> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .map(|e| e.path())
            .filter(|p| {
                p.file_name()
                    .and_then(|s| s.to_str())
                    .map(|n| n.starts_with("unit.") && n.ends_with(".log"))
                    .unwrap_or(false)
            })
            .collect();

        assert!(
            entries.len() >= 2,
            "expected rotated + current, got {entries:?}"
        );
    }

    #[test]
    fn retention_caps_rolled_files() {
        let dir = tmp_dir("retain");
        // Seed 6 bogus rolled files, then prune to max_files = 2.
        for i in 0..6 {
            let name = format!("unit.2020010{i}T000000.log");
            std::fs::write(dir.join(&name), format!("content {i}")).unwrap();
        }
        // Plus one "current" file so prune_old keeps it.
        std::fs::write(
            dir.join(format!(
                "unit.{}.log",
                CalendarDate::today_local().as_basename()
            )),
            b"current",
        )
        .unwrap();

        prune_old(&dir, "unit", 2);

        let rolled: Vec<String> = std::fs::read_dir(&dir)
            .unwrap()
            .flatten()
            .filter_map(|e| {
                let n = e.file_name().to_string_lossy().into_owned();
                if n.starts_with("unit.") && n.ends_with(".log") && n.contains('T')
                // timestamped = rolled
                {
                    Some(n)
                } else {
                    None
                }
            })
            .collect();
        assert_eq!(rolled.len(), 2, "kept: {rolled:?}");
    }

    #[test]
    fn init_and_shutdown_are_idempotent() {
        let dir = tmp_dir("install");
        let cfg = FileLoggingConfig {
            directory: Some(dir.clone()),
            file_name_prefix: "install".to_string(),
            max_size_bytes: 1024,
            max_files: 2,
            rotation: RotationPolicy::Both,
            include_console: true,
        };

        let resolved = init_file_logging(cfg.clone()).unwrap();
        assert_eq!(resolved, dir);

        // Swap — should not panic.
        let resolved2 = init_file_logging(cfg).unwrap();
        assert_eq!(resolved2, dir);

        shutdown_file_logging().unwrap();
        // Second shutdown is a no-op.
        shutdown_file_logging().unwrap();
    }
}
