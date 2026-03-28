//! Filesystem utilities — replaces platformdirs with the `dirs` crate.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use crate::constants::{APP_NAME, ENV_SKILL_PATHS};
use std::env;
use std::path::{Path, PathBuf};

/// Get a platform-specific directory path.
pub fn get_platform_dir(dir_type: &str) -> Result<String, String> {
    let base = match dir_type {
        "config" => dirs::config_dir(),
        "data" => dirs::data_dir(),
        "cache" => dirs::cache_dir(),
        "log" | "state" => dirs::data_local_dir(),
        "documents" => dirs::document_dir(),
        _ => return Err(format!("Unknown directory type: {}", dir_type)),
    };

    let base_dir = base.ok_or_else(|| format!("Cannot determine {} directory", dir_type))?;
    let dir = base_dir.join(APP_NAME);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create directory: {}", e))?;
    Ok(dir.to_string_lossy().to_string())
}

/// Get config directory.
pub fn get_config_dir() -> Result<String, String> {
    get_platform_dir("config")
}

/// Get data directory.
pub fn get_data_dir() -> Result<String, String> {
    get_platform_dir("data")
}

/// Get log directory.
pub fn get_log_dir() -> Result<String, String> {
    let base =
        dirs::data_local_dir().ok_or_else(|| "Cannot determine log directory".to_string())?;
    let dir = base.join(APP_NAME).join("log");
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create log directory: {}", e))?;
    Ok(dir.to_string_lossy().to_string())
}

/// Get DCC-specific actions directory.
pub fn get_actions_dir(dcc_name: &str) -> Result<String, String> {
    let data_dir = get_data_dir()?;
    let dir = PathBuf::from(&data_dir).join("actions").join(dcc_name);
    std::fs::create_dir_all(&dir).map_err(|e| format!("Cannot create actions directory: {}", e))?;
    Ok(dir.to_string_lossy().to_string())
}

/// Get skills directory.
pub fn get_skills_dir(dcc_name: Option<&str>) -> Result<String, String> {
    let data_dir = get_data_dir()?;
    let dir = if let Some(dcc) = dcc_name {
        PathBuf::from(&data_dir)
            .join("skills")
            .join(dcc.to_lowercase())
    } else {
        PathBuf::from(&data_dir).join("skills")
    };
    // Don't create — just return path (caller may check existence)
    Ok(dir.to_string_lossy().to_string())
}

/// Get skill search paths from environment variable.
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

/// Ensure a directory exists.
pub fn ensure_directory_exists(dir_path: &str) -> bool {
    std::fs::create_dir_all(dir_path).is_ok()
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_platform_dir")]
pub fn py_get_platform_dir(dir_type: &str) -> PyResult<String> {
    get_platform_dir(dir_type).map_err(|e| pyo3::exceptions::PyValueError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_config_dir")]
pub fn py_get_config_dir() -> PyResult<String> {
    get_config_dir().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_data_dir")]
pub fn py_get_data_dir() -> PyResult<String> {
    get_data_dir().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_log_dir")]
pub fn py_get_log_dir() -> PyResult<String> {
    get_log_dir().map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_actions_dir")]
pub fn py_get_actions_dir(dcc_name: &str) -> PyResult<String> {
    get_actions_dir(dcc_name).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_skills_dir")]
#[pyo3(signature = (dcc_name=None))]
pub fn py_get_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
    get_skills_dir(dcc_name).map_err(|e| pyo3::exceptions::PyRuntimeError::new_err(e))
}

#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "get_skill_paths_from_env")]
pub fn py_get_skill_paths_from_env() -> Vec<String> {
    get_skill_paths_from_env()
}

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
    }

    #[test]
    fn test_get_skill_paths_empty() {
        // Unless env var is set, should return empty
        let paths = get_skill_paths_from_env();
        // May or may not be empty depending on env
        let _ = paths;
    }
}
