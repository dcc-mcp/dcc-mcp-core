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
///
/// Reads `DCC_MCP_SKILL_PATHS` (colon-separated on Unix, semicolon on Windows).
/// Only returns paths that exist on the filesystem.
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

/// Get skill search paths for a specific app from environment variables.
///
/// Reads **both** paths in priority order (most specific first):
/// 1. `DCC_MCP_{APP}_SKILL_PATHS` — per-app paths (e.g. `DCC_MCP_MAYA_SKILL_PATHS`)
/// 2. `DCC_MCP_SKILL_PATHS` — global fallback
///
/// Paths are deduplicated while preserving order. Only existing directories
/// are returned.
///
/// # Examples
///
/// ```no_run
/// use dcc_mcp_utils::filesystem::get_app_skill_paths_from_env;
///
/// // With DCC_MCP_MAYA_SKILL_PATHS=/studio/maya-skills
/// // and  DCC_MCP_SKILL_PATHS=/shared/skills
/// let paths = get_app_skill_paths_from_env("maya");
/// // → ["/studio/maya-skills", "/shared/skills"]  (deduped, existing only)
/// ```
#[must_use]
pub fn get_app_skill_paths_from_env(app_name: &str) -> Vec<String> {
    use crate::constants::app_skill_paths_env_key;

    let sep = if cfg!(windows) { ';' } else { ':' };
    let mut seen = std::collections::HashSet::new();
    let mut paths = Vec::new();

    let mut add = |value: &str| {
        for p in value.split(sep) {
            let p = p.trim();
            if !p.is_empty() && Path::new(p).is_dir() && seen.insert(p.to_string()) {
                paths.push(p.to_string());
            }
        }
    };

    // 1. Per-app paths (highest priority)
    let app_key = app_skill_paths_env_key(app_name);
    if let Ok(v) = env::var(&app_key) {
        add(&v);
    }

    // 2. Global fallback
    if let Ok(v) = env::var(ENV_SKILL_PATHS) {
        add(&v);
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

    #[pyfunction]
    #[pyo3(name = "get_app_skill_paths_from_env")]
    pub fn py_get_app_skill_paths_from_env(app_name: &str) -> Vec<String> {
        get_app_skill_paths_from_env(app_name)
    }
}

#[cfg(feature = "python-bindings")]
pub use py_bindings::*;

#[cfg(test)]
mod tests {
    use super::*;

    // ── platform_dir ────────────────────────────────────────────────────────────

    #[test]
    fn test_get_platform_dir_config() {
        let result = get_platform_dir("config");
        assert!(result.is_ok());
        assert!(result.unwrap().contains("dcc-mcp"));
    }

    #[test]
    fn test_get_platform_dir_data() {
        let result = get_platform_dir("data");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_platform_dir_cache() {
        let result = get_platform_dir("cache");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_platform_dir_log() {
        let result = get_platform_dir("log");
        assert!(result.is_ok());
    }

    #[test]
    fn test_get_platform_dir_state() {
        let result = get_platform_dir("state");
        assert!(result.is_ok());
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
        let result = get_platform_dir("");
        assert!(result.is_err());
    }

    // ── convenience wrappers ────────────────────────────────────────────────────

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
    fn test_get_actions_dir() {
        let result = get_actions_dir("maya");
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.contains("maya"));
        assert!(path.contains("actions"));
    }

    #[test]
    fn test_get_actions_dir_different_dccs() {
        for dcc in &["blender", "houdini", "3dsmax", "unreal"] {
            let result = get_actions_dir(dcc);
            assert!(result.is_ok(), "Failed for dcc={dcc}");
            assert!(result.unwrap().contains(dcc));
        }
    }

    // ── skills dir ──────────────────────────────────────────────────────────────

    #[test]
    fn test_get_skills_dir_no_dcc() {
        let result = get_skills_dir(None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("skills"));
    }

    #[test]
    fn test_get_skills_dir_with_dcc() {
        let result = get_skills_dir(Some("maya"));
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.contains("maya"));
        assert!(path.contains("skills"));
    }

    #[test]
    fn test_get_skills_dir_dcc_is_lowercase() {
        let lower = get_skills_dir(Some("blender")).unwrap();
        let upper = get_skills_dir(Some("BLENDER")).unwrap();
        // Both should produce lowercase dcc in path
        assert!(lower.contains("blender"));
        assert!(upper.contains("blender"));
    }

    // ── path_to_string ──────────────────────────────────────────────────────────

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

    // ── FilesystemError ─────────────────────────────────────────────────────────

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

    // ── get_skill_paths_from_env ────────────────────────────────────────────────

    #[test]
    fn test_get_skill_paths_from_env_returns_vec() {
        let paths = get_skill_paths_from_env();
        // Without DCC_MCP_SKILL_PATHS set, result should be empty.
        // If the env var happens to be set in CI, just verify the type.
        assert!(paths.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn test_get_skill_paths_from_env_with_temp_dir() {
        use std::env;
        use std::fs;
        // Create a real temp dir so is_dir() returns true
        let tmp = env::temp_dir().join("dcc_mcp_test_skill_path");
        let _ = fs::create_dir_all(&tmp);
        let tmp_str = tmp.to_string_lossy().to_string();

        let sep = if cfg!(windows) { ';' } else { ':' };
        // SAFETY: single-threaded test, no other thread reads this env var.
        unsafe {
            env::set_var(ENV_SKILL_PATHS, &tmp_str);
        }
        let paths = get_skill_paths_from_env();
        // SAFETY: restore env to avoid leaking state.
        unsafe {
            env::remove_var(ENV_SKILL_PATHS);
        }
        let _ = fs::remove_dir(&tmp);

        // The temp dir should appear in the returned paths
        assert!(
            paths.iter().any(|p| p.contains("dcc_mcp_test_skill_path")),
            "Expected temp dir in paths (sep={sep:?}), got: {paths:?}"
        );
    }
}
