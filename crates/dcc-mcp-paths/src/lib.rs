//! # dcc-mcp-paths
//!
//! Cross-platform application directory helpers for the DCC-MCP ecosystem.
//!
//! Backed by the [`dirs`] crate. Resolves and creates standard
//! per-user directories under a single [`APP_NAME`]-rooted subtree:
//!
//! * [`get_config_dir`] — `$XDG_CONFIG_HOME/dcc-mcp` (Linux) /
//!   `%APPDATA%\dcc-mcp` (Windows) / `~/Library/Application Support/dcc-mcp` (macOS)
//! * [`get_data_dir`] — `$XDG_DATA_HOME/dcc-mcp` etc.
//! * [`get_log_dir`] — local data dir + `log/`
//! * [`get_tools_dir`] — DCC-scoped action storage under data dir
//!
//! All helpers are infallible from the API perspective (return
//! [`FilesystemError`]) and create the directory as a side-effect.
//!
//! Skill-domain path resolution (search-path env vars, scope-aware
//! discovery, `copy_skill_to_*_dir`) lives in
//! [`dcc-mcp-skills`](https://docs.rs/dcc-mcp-skills) — this crate is
//! intentionally restricted to the platform-dir building blocks.

use std::path::{Path, PathBuf};

/// Application name used for platform-specific directory resolution.
pub const APP_NAME: &str = "dcc-mcp";
/// Application author identifier (exposed to Python consumers).
pub const APP_AUTHOR: &str = "dcc-mcp";

/// Structured error type for filesystem operations.
#[derive(Debug)]
#[non_exhaustive]
pub enum FilesystemError {
    /// The requested directory type is not recognized.
    UnknownDirType(String),
    /// The platform-specific base directory could not be determined.
    PlatformDirNotFound(String),
    /// An I/O error occurred (e.g. directory creation failed).
    Io(std::io::Error),
}

impl std::fmt::Display for FilesystemError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::UnknownDirType(t) => write!(f, "Unknown directory type: {t}"),
            Self::PlatformDirNotFound(t) => write!(f, "Cannot determine {t} directory"),
            Self::Io(e) => write!(f, "Filesystem I/O error: {e}"),
        }
    }
}

impl std::error::Error for FilesystemError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        match self {
            Self::Io(e) => Some(e),
            _ => None,
        }
    }
}

impl From<std::io::Error> for FilesystemError {
    fn from(err: std::io::Error) -> Self {
        Self::Io(err)
    }
}

impl From<serde_json::Error> for FilesystemError {
    fn from(err: serde_json::Error) -> Self {
        Self::Io(std::io::Error::new(std::io::ErrorKind::InvalidData, err))
    }
}

#[cfg(feature = "python-bindings")]
impl From<FilesystemError> for pyo3::PyErr {
    fn from(err: FilesystemError) -> pyo3::PyErr {
        match err {
            FilesystemError::UnknownDirType(_) => {
                pyo3::exceptions::PyValueError::new_err(err.to_string())
            }
            FilesystemError::PlatformDirNotFound(_) | FilesystemError::Io(_) => {
                pyo3::exceptions::PyOSError::new_err(err.to_string())
            }
        }
    }
}

// ── Internal PathBuf helpers (no String↔PathBuf round-trips) ──

fn platform_dir_path(dir_type: &str) -> Result<PathBuf, FilesystemError> {
    let base = match dir_type {
        "config" => dirs::config_dir(),
        "data" => dirs::data_dir(),
        "cache" => dirs::cache_dir(),
        "log" | "state" => dirs::data_local_dir(),
        "documents" => dirs::document_dir(),
        _ => return Err(FilesystemError::UnknownDirType(dir_type.to_string())),
    };
    let dir = base
        .ok_or_else(|| FilesystemError::PlatformDirNotFound(dir_type.to_string()))?
        .join(APP_NAME);
    std::fs::create_dir_all(&dir)?;
    Ok(dir)
}

fn data_dir_path() -> Result<PathBuf, FilesystemError> {
    platform_dir_path("data")
}

/// Convert a [`Path`] to a lossy `String`.
#[must_use]
pub fn path_to_string(p: &Path) -> String {
    p.to_string_lossy().into_owned()
}

/// Get a platform-specific directory path.
///
/// # Errors
/// Returns [`FilesystemError`] if the directory type is unknown or the platform dir cannot be determined.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_platform_dir(dir_type: &str) -> Result<String, FilesystemError> {
    platform_dir_path(dir_type).map(|p| path_to_string(&p))
}

/// Get config directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform config dir cannot be determined.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_config_dir() -> Result<String, FilesystemError> {
    get_platform_dir("config")
}

/// Get data directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_data_dir() -> Result<String, FilesystemError> {
    get_platform_dir("data")
}

/// Get log directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform dir cannot be determined or directory creation fails.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_log_dir() -> Result<String, FilesystemError> {
    let dir = platform_dir_path("log")?.join("log");
    std::fs::create_dir_all(&dir)?;
    Ok(path_to_string(&dir))
}

/// Get DCC-specific tools directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined or directory creation fails.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_tools_dir(dcc_name: &str) -> Result<String, FilesystemError> {
    let dir = data_dir_path()?.join("actions").join(dcc_name);
    std::fs::create_dir_all(&dir)?;
    Ok(path_to_string(&dir))
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
mod py_bindings {
    use super::*;
    use pyo3::prelude::*;

    macro_rules! py_dir_binding {
        ($py_fn:ident, $pyo3_name:literal, $rust_fn:ident) => {
            #[pyfunction]
            #[pyo3(name = $pyo3_name)]
            pub fn $py_fn() -> PyResult<String> {
                Ok($rust_fn()?)
            }
        };
        ($py_fn:ident, $pyo3_name:literal, $rust_fn:ident, $arg:ident : $ty:ty) => {
            #[pyfunction]
            #[pyo3(name = $pyo3_name)]
            pub fn $py_fn($arg: $ty) -> PyResult<String> {
                Ok($rust_fn($arg)?)
            }
        };
    }

    py_dir_binding!(py_get_config_dir, "get_config_dir", get_config_dir);
    py_dir_binding!(py_get_data_dir, "get_data_dir", get_data_dir);
    py_dir_binding!(py_get_log_dir, "get_log_dir", get_log_dir);
    py_dir_binding!(py_get_platform_dir, "get_platform_dir", get_platform_dir, dir_type: &str);
    py_dir_binding!(py_get_tools_dir, "get_tools_dir", get_tools_dir, dcc_name: &str);
}

#[cfg(feature = "python-bindings")]
pub use py_bindings::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_app_name_not_empty() {
        assert_eq!(APP_NAME, "dcc-mcp");
    }

    #[test]
    fn test_get_platform_dir_config() {
        let result = get_platform_dir("config");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("dcc-mcp"));
    }

    #[test]
    fn test_get_platform_dir_data() {
        assert!(get_platform_dir("data").is_ok());
    }

    #[test]
    fn test_get_platform_dir_cache() {
        assert!(get_platform_dir("cache").is_ok());
    }

    #[test]
    fn test_get_platform_dir_log() {
        assert!(get_platform_dir("log").is_ok());
    }

    #[test]
    fn test_get_platform_dir_state() {
        assert!(get_platform_dir("state").is_ok());
    }

    #[test]
    fn test_get_platform_dir_invalid() {
        let result = get_platform_dir("invalid_type");
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            FilesystemError::UnknownDirType(_)
        ));
    }

    #[test]
    fn test_get_platform_dir_empty_type() {
        assert!(get_platform_dir("").is_err());
    }

    #[test]
    fn test_get_config_dir() {
        assert!(get_config_dir().is_ok());
    }

    #[test]
    fn test_get_data_dir() {
        assert!(get_data_dir().is_ok());
    }

    #[test]
    fn test_get_log_dir() {
        let result = get_log_dir();
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.contains("log"));
    }

    #[test]
    fn test_get_tools_dir() {
        let result = get_tools_dir("maya");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.contains("maya"));
        assert!(path.contains("actions"));
    }

    #[test]
    fn test_get_tools_dir_different_dccs() {
        for dcc in &["blender", "houdini", "3dsmax", "unreal"] {
            let result = get_tools_dir(dcc);
            assert!(result.is_ok(), "Failed for dcc={dcc}");
            assert!(result.unwrap().contains(dcc));
        }
    }

    #[test]
    fn test_path_to_string_basic() {
        let p = PathBuf::from("/some/path");
        let s = path_to_string(&p);
        assert!(s.contains("some"));
        assert!(s.contains("path"));
    }

    #[test]
    fn test_path_to_string_empty() {
        let p = PathBuf::from("");
        let s = path_to_string(&p);
        assert_eq!(s, "");
    }

    #[test]
    fn test_filesystem_error_display() {
        let err = FilesystemError::UnknownDirType("foo".to_string());
        assert_eq!(err.to_string(), "Unknown directory type: foo");

        let err = FilesystemError::PlatformDirNotFound("data".to_string());
        assert_eq!(err.to_string(), "Cannot determine data directory");
    }

    #[test]
    fn test_filesystem_error_io_display() {
        let io_err = std::io::Error::new(std::io::ErrorKind::NotFound, "file missing");
        let fs_err = FilesystemError::Io(io_err);
        assert!(fs_err.to_string().contains("Filesystem I/O error"));
    }

    #[test]
    fn test_filesystem_error_source() {
        use std::error::Error;

        let io_err = std::io::Error::new(std::io::ErrorKind::PermissionDenied, "denied");
        let fs_err = FilesystemError::Io(io_err);
        assert!(fs_err.source().is_some());

        let no_source = FilesystemError::UnknownDirType("x".to_string());
        assert!(no_source.source().is_none());
    }

    #[test]
    fn test_filesystem_error_from_io() {
        let io_err = std::io::Error::other("other");
        let fs_err: FilesystemError = io_err.into();
        assert!(matches!(fs_err, FilesystemError::Io(_)));
    }
}
