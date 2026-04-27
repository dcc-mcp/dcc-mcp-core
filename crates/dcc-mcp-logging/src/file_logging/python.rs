//! PyO3 wrappers for [`super::FileLoggingConfig`] and the install /
//! shutdown / flush entry points.

use super::config::{FileLoggingConfig, RotationPolicy};
use super::{init_file_logging, shutdown_file_logging};
use crate::constants::{
    DEFAULT_LOG_FILE_PREFIX, DEFAULT_LOG_MAX_FILES, DEFAULT_LOG_MAX_SIZE, DEFAULT_LOG_ROTATION,
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

/// Flush buffered log events to disk immediately.
///
/// `tracing_appender::non_blocking` batches writes on a background thread
/// and only guarantees a flush on rotation or process exit. For
/// long-running DCC sessions this means the log file can appear empty or
/// stale. Call `flush_logs()` to force all buffered events to disk now —
/// useful after an error, before opening the log viewer, or from a
/// periodic Python timer. Issue #402.
#[pyfunction]
#[pyo3(name = "flush_logs")]
pub fn py_flush_logs() -> PyResult<()> {
    super::flush_logs().map_err(pyo3::exceptions::PyOSError::new_err)?;
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
