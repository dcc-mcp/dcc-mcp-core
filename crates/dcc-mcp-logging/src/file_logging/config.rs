//! Configuration and error types for [`super`].

use crate::config::FileLayerInstallError;
use crate::constants::{
    DEFAULT_LOG_FILE_PREFIX, DEFAULT_LOG_MAX_FILES, DEFAULT_LOG_MAX_SIZE, DEFAULT_LOG_ROTATION,
    ENV_LOG_DIR, ENV_LOG_FILE, ENV_LOG_FILE_PREFIX, ENV_LOG_MAX_FILES, ENV_LOG_MAX_SIZE,
    ENV_LOG_ROTATION,
};
use dcc_mcp_utils::filesystem::get_log_dir;

use std::io;
use std::path::PathBuf;

// ── Rotation policy ──────────────────────────────────────────────────────────

/// Rotation policy used by [`super::RollingFileWriter`].
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

    pub(crate) fn rotates_on_size(self) -> bool {
        matches!(self, Self::Size | Self::Both)
    }

    pub(crate) fn rotates_on_date(self) -> bool {
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

// ── FileLoggingConfig ────────────────────────────────────────────────────────

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
    /// [`crate::config::init_logging`] and is always installed; this
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

    pub(crate) fn resolved_directory(&self) -> Result<PathBuf, FileLoggingError> {
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
    /// The reload mechanism in [`crate::config`] is not yet initialized
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
