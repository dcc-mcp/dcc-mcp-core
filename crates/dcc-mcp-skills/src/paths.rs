//! Skill-search-path resolution and skill-scope directories.
//!
//! Skill-specific layer over [`dcc-mcp-paths`]. The platform-dir helpers
//! (`get_data_dir`, `get_config_dir`, …) and [`FilesystemError`] live in
//! [`dcc-mcp-paths`] because they are not skill-specific; this module
//! adds the SKILL.md / scope-aware semantics on top.
//!
//! [`FilesystemError`]: dcc_mcp_paths::FilesystemError

use crate::constants::{
    ENV_SKILL_PATHS, ENV_TEAM_SKILL_PATHS, ENV_USER_SKILL_PATHS, SKILL_METADATA_FILE,
    app_skill_paths_env_key, team_skill_paths_env_key, user_skill_paths_env_key,
};
use dcc_mcp_paths::{FilesystemError, get_data_dir, path_to_string};
use std::env;
use std::path::{Path, PathBuf};

/// Get skills directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined.
#[must_use = "this returns the skills directory path; the directory is not created"]
pub fn get_skills_dir(dcc_name: Option<&str>) -> Result<String, FilesystemError> {
    let mut dir = PathBuf::from(get_data_dir()?).join("skills");
    if let Some(dcc) = dcc_name {
        dir = dir.join(dcc.to_lowercase());
    }
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
/// use dcc_mcp_skills::paths::get_app_skill_paths_from_env;
///
/// // With DCC_MCP_MAYA_SKILL_PATHS=/studio/maya-skills
/// // and  DCC_MCP_SKILL_PATHS=/shared/skills
/// let paths = get_app_skill_paths_from_env("maya");
/// // → ["/studio/maya-skills", "/shared/skills"]  (deduped, existing only)
/// ```
#[must_use]
pub fn get_app_skill_paths_from_env(app_name: &str) -> Vec<String> {
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

    let app_key = app_skill_paths_env_key(app_name);
    if let Ok(v) = env::var(&app_key) {
        add(&v);
    }

    if let Ok(v) = env::var(ENV_SKILL_PATHS) {
        add(&v);
    }

    paths
}

/// Get user-level accumulated skill search paths from environment variable.
///
/// Reads `DCC_MCP_USER_SKILL_PATHS` (colon-separated on Unix, semicolon on Windows).
/// Only returns paths that exist on the filesystem.
#[must_use]
pub fn get_user_skill_paths_from_env() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(value) = env::var(ENV_USER_SKILL_PATHS) {
        for p in value.split(if cfg!(windows) { ';' } else { ':' }) {
            let p = p.trim();
            if !p.is_empty() && Path::new(p).is_dir() {
                paths.push(p.to_string());
            }
        }
    }
    paths
}

/// Get team-level accumulated skill search paths from environment variable.
///
/// Reads `DCC_MCP_TEAM_SKILL_PATHS` (colon-separated on Unix, semicolon on Windows).
/// Only returns paths that exist on the filesystem.
#[must_use]
pub fn get_team_skill_paths_from_env() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(value) = env::var(ENV_TEAM_SKILL_PATHS) {
        for p in value.split(if cfg!(windows) { ';' } else { ':' }) {
            let p = p.trim();
            if !p.is_empty() && Path::new(p).is_dir() {
                paths.push(p.to_string());
            }
        }
    }
    paths
}

/// Get user-level accumulated skill search paths for a specific app.
///
/// Reads **both** paths in priority order (most specific first):
/// 1. `DCC_MCP_USER_{APP}_SKILL_PATHS` — per-app paths
/// 2. `DCC_MCP_USER_SKILL_PATHS` — global fallback
///
/// Paths are deduplicated while preserving order. Only existing directories
/// are returned.
#[must_use]
pub fn get_app_user_skill_paths_from_env(app_name: &str) -> Vec<String> {
    accumulate_app_paths(&user_skill_paths_env_key(app_name), ENV_USER_SKILL_PATHS)
}

/// Get team-level accumulated skill search paths for a specific app.
///
/// Reads **both** paths in priority order (most specific first):
/// 1. `DCC_MCP_TEAM_{APP}_SKILL_PATHS` — per-app paths
/// 2. `DCC_MCP_TEAM_SKILL_PATHS` — global fallback
///
/// Paths are deduplicated while preserving order. Only existing directories
/// are returned.
#[must_use]
pub fn get_app_team_skill_paths_from_env(app_name: &str) -> Vec<String> {
    accumulate_app_paths(&team_skill_paths_env_key(app_name), ENV_TEAM_SKILL_PATHS)
}

/// Internal helper: accumulate per-app + scope-fallback paths, deduped.
fn accumulate_app_paths(app_key: &str, fallback_key: &str) -> Vec<String> {
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

    if let Ok(v) = env::var(app_key) {
        add(&v);
    }
    if let Ok(v) = env::var(fallback_key) {
        add(&v);
    }
    paths
}

/// Get user-level accumulated skills directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_user_skills_dir(dcc_name: Option<&str>) -> Result<String, FilesystemError> {
    let mut dir = PathBuf::from(get_data_dir()?).join("skills").join("user");
    if let Some(dcc) = dcc_name {
        dir = dir.join(dcc.to_lowercase());
    }
    std::fs::create_dir_all(&dir)?;
    Ok(path_to_string(&dir))
}

/// Get team-level accumulated skills directory.
///
/// # Errors
/// Returns [`FilesystemError`] if the platform data dir cannot be determined.
#[must_use = "this returns the directory path and also creates it as a side effect"]
pub fn get_team_skills_dir(dcc_name: Option<&str>) -> Result<String, FilesystemError> {
    let mut dir = PathBuf::from(get_data_dir()?).join("skills").join("team");
    if let Some(dcc) = dcc_name {
        dir = dir.join(dcc.to_lowercase());
    }
    std::fs::create_dir_all(&dir)?;
    Ok(path_to_string(&dir))
}

/// Copy a skill directory to the user-level accumulated skills directory.
///
/// Validates that `src` contains a `SKILL.md` file.
/// Returns the destination path.
///
/// # Errors
/// Returns [`FilesystemError`] if the source is invalid or the copy fails.
pub fn copy_skill_to_user_dir(
    src: &Path,
    dcc_name: Option<&str>,
) -> Result<PathBuf, FilesystemError> {
    copy_skill_to_scope_dir(src, &get_user_skills_dir(dcc_name)?)
}

/// Copy a skill directory to the team-level accumulated skills directory.
///
/// Validates that `src` contains a `SKILL.md` file.
/// Returns the destination path.
///
/// # Errors
/// Returns [`FilesystemError`] if the source is invalid or the copy fails.
pub fn copy_skill_to_team_dir(
    src: &Path,
    dcc_name: Option<&str>,
) -> Result<PathBuf, FilesystemError> {
    copy_skill_to_scope_dir(src, &get_team_skills_dir(dcc_name)?)
}

fn copy_skill_to_scope_dir(src: &Path, dest_base: &str) -> Result<PathBuf, FilesystemError> {
    if !src.is_dir() {
        return Err(FilesystemError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("source is not a directory: {}", src.display()),
        )));
    }
    let skill_md = src.join(SKILL_METADATA_FILE);
    if !skill_md.is_file() {
        return Err(FilesystemError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "source directory missing {}: {}",
                SKILL_METADATA_FILE,
                src.display()
            ),
        )));
    }

    let skill_name = src
        .file_name()
        .and_then(|n| n.to_str())
        .unwrap_or("unknown");
    let dest = Path::new(dest_base).join(skill_name);

    // If destination already exists, archive current version before overwrite.
    if dest.exists() {
        crate::versioning::archive_skill_version(&dest, None)?;
        // Remove existing (except .versions and .feedback.jsonl)
        for entry in std::fs::read_dir(&dest)? {
            let entry = entry?;
            let name = entry.file_name();
            let name_str = name.to_string_lossy();
            if name_str == ".versions" || name_str == ".feedback.jsonl" {
                continue;
            }
            let path = entry.path();
            if path.is_dir() {
                std::fs::remove_dir_all(&path)?;
            } else {
                std::fs::remove_file(&path)?;
            }
        }
        // Now copy new content into the existing directory
        for entry in std::fs::read_dir(src)? {
            let entry = entry?;
            let src_path = entry.path();
            let dest_path = dest.join(entry.file_name());
            if src_path.is_dir() {
                copy_dir_recursive(&src_path, &dest_path)?;
            } else {
                std::fs::copy(&src_path, &dest_path)?;
            }
        }
    } else {
        copy_dir_recursive(src, &dest)?;
    }

    crate::versioning::update_version_manifest(&dest, skill_name)?;

    Ok(dest)
}

pub(crate) fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
    std::fs::create_dir_all(dst)?;
    for entry in std::fs::read_dir(src)? {
        let entry = entry?;
        let path = entry.path();
        let dest_path = dst.join(entry.file_name());
        if path.is_dir() {
            copy_dir_recursive(&path, &dest_path)?;
        } else {
            std::fs::copy(&path, &dest_path)?;
        }
    }
    Ok(())
}

// ── Python bindings ──

#[cfg(feature = "python-bindings")]
mod py_bindings {
    use super::*;
    use dcc_mcp_paths::path_to_string;
    use pyo3::prelude::*;

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

    #[pyfunction]
    #[pyo3(name = "get_user_skill_paths_from_env")]
    pub fn py_get_user_skill_paths_from_env() -> Vec<String> {
        get_user_skill_paths_from_env()
    }

    #[pyfunction]
    #[pyo3(name = "get_team_skill_paths_from_env")]
    pub fn py_get_team_skill_paths_from_env() -> Vec<String> {
        get_team_skill_paths_from_env()
    }

    #[pyfunction]
    #[pyo3(name = "get_app_user_skill_paths_from_env")]
    pub fn py_get_app_user_skill_paths_from_env(app_name: &str) -> Vec<String> {
        get_app_user_skill_paths_from_env(app_name)
    }

    #[pyfunction]
    #[pyo3(name = "get_app_team_skill_paths_from_env")]
    pub fn py_get_app_team_skill_paths_from_env(app_name: &str) -> Vec<String> {
        get_app_team_skill_paths_from_env(app_name)
    }

    #[pyfunction]
    #[pyo3(name = "get_user_skills_dir")]
    #[pyo3(signature = (dcc_name=None))]
    pub fn py_get_user_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
        Ok(get_user_skills_dir(dcc_name)?)
    }

    #[pyfunction]
    #[pyo3(name = "get_team_skills_dir")]
    #[pyo3(signature = (dcc_name=None))]
    pub fn py_get_team_skills_dir(dcc_name: Option<&str>) -> PyResult<String> {
        Ok(get_team_skills_dir(dcc_name)?)
    }

    #[pyfunction]
    #[pyo3(name = "copy_skill_to_user_dir")]
    #[pyo3(signature = (src, dcc_name=None))]
    pub fn py_copy_skill_to_user_dir(src: &str, dcc_name: Option<&str>) -> PyResult<String> {
        let dest = copy_skill_to_user_dir(Path::new(src), dcc_name)?;
        Ok(path_to_string(&dest))
    }

    #[pyfunction]
    #[pyo3(name = "copy_skill_to_team_dir")]
    #[pyo3(signature = (src, dcc_name=None))]
    pub fn py_copy_skill_to_team_dir(src: &str, dcc_name: Option<&str>) -> PyResult<String> {
        let dest = copy_skill_to_team_dir(Path::new(src), dcc_name)?;
        Ok(path_to_string(&dest))
    }
}

#[cfg(feature = "python-bindings")]
pub use py_bindings::*;

#[cfg(test)]
mod tests {
    use super::*;

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
        assert!(lower.contains("blender"));
        assert!(upper.contains("blender"));
    }

    #[test]
    fn test_get_skill_paths_from_env_returns_vec() {
        let paths = get_skill_paths_from_env();
        assert!(paths.iter().all(|p| !p.is_empty()));
    }

    #[test]
    fn test_get_skill_paths_from_env_with_temp_dir() {
        use std::fs;
        let tmp = env::temp_dir().join("dcc_mcp_skills_test_skill_path");
        let _ = fs::create_dir_all(&tmp);
        let tmp_str = tmp.to_string_lossy().to_string();

        let sep = if cfg!(windows) { ';' } else { ':' };
        // SAFETY: single-threaded test; the env-mutation is contained.
        unsafe {
            env::set_var(ENV_SKILL_PATHS, &tmp_str);
        }
        let paths = get_skill_paths_from_env();
        // SAFETY: restore env to avoid leaking state.
        unsafe {
            env::remove_var(ENV_SKILL_PATHS);
        }
        let _ = fs::remove_dir(&tmp);

        assert!(
            paths
                .iter()
                .any(|p| p.contains("dcc_mcp_skills_test_skill_path")),
            "Expected temp dir in paths (sep={sep:?}), got: {paths:?}"
        );
    }
}
