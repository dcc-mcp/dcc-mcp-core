//! Filesystem utilities — replaces platformdirs with the `dirs` crate.

use crate::constants::{APP_NAME, ENV_SKILL_PATHS};
use std::env;
use std::path::{Path, PathBuf};

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

/// Get DCC-specific actions directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined or directory creation fails.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_actions_dir(dcc_name: &str) -> Result<String, FilesystemError> {
    let dir = data_dir_path()?.join("actions").join(dcc_name);
    std::fs::create_dir_all(&dir)?;
    Ok(path_to_string(&dir))
}

/// Get skills directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined.
#[must_use = "this returns the skills directory path; the directory is not created"]
pub fn get_skills_dir(dcc_name: Option<&str>) -> Result<String, FilesystemError> {
    let mut dir = data_dir_path()?.join("skills");
    if let Some(dcc) = dcc_name {
        dir = dir.join(dcc.to_lowercase());
    }
    // Don't create — just return path (caller may check existence)
    Ok(path_to_string(&dir))
}

/// Get skill search paths from environment variable.
#[must_use]
pub fn get_skill_paths_from_env() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(value) = env::var(ENV_SKILL_PATHS) {
        for p in value.split(if cfg!(windows) { ';' } else { ':' }) {
            let p = p.trim();
            if !p.is_empty() && Path::new(p).is_dir() {
                paths.push(p.to_string());
            }
        }
    }
    paths
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
mod py_bindings {
    use super::*;
    use pyo3::prelude::*;

    /// Generate a `#[pyfunction]` that delegates to a Rust function returning `Result<String, _>`.
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
    py_dir_binding!(py_get_actions_dir, "get_actions_dir", get_actions_dir, dcc_name: &str);

    // get_skills_dir has a custom pyo3 signature (optional param), kept manual.
    #[pyfunction]
    #[pyo3(name = "get_skills_dir")]
    #[pyo3(signature = (dcc_name=None))]
    pub fn py_get_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
        Ok(get_skills_dir(dcc_name)?)
    }

    #[pyfunction]
    #[pyo3(name = "get_skill_paths_from_env")]
    pub fn py_get_skill_paths_from_env() -> Vec<String> {
        get_skill_paths_from_env()
    }
}

#[cfg(feature = "python-bindings")]
pub use py_bindings::*;

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_get_platform_dir() {
        let result = get_platform_dir("config");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("dcc-mcp"));
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
    fn test_filesystem_error_display() {
        let err = FilesystemError::UnknownDirType("foo".to_string());
        assert_eq!(err.to_string(), "Unknown directory type: foo");

        let err = FilesystemError::PlatformDirNotFound("data".to_string());
        assert_eq!(err.to_string(), "Cannot determine data directory");
    }

    #[test]
    fn test_get_skill_paths_from_env_returns_vec() {
        let paths = get_skill_paths_from_env();
        // Without DCC_MCP_SKILL_PATHS set, result should be empty.
        // If the env var happens to be set in CI, just verify the type.
        assert!(paths.iter().all(|p| !p.is_empty()));
    }
}
