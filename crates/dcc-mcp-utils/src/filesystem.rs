//! Filesystem utilities — replaces platformdirs with the `dirs` crate.

use crate::constants::{APP_NAME, ENV_SKILL_PATHS};
use std::env;
use std::path::{Path, PathBuf};

/// Feedback entry for an evolved skill execution.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillFeedback", get_all, set_all)
)]
pub struct SkillFeedback {
    /// ISO-8601 timestamp.
    pub timestamp: String,
    /// Whether the skill execution succeeded.
    pub success: bool,
    /// Optional user/agent correction or improved prompt.
    pub correction: Option<String>,
    /// Optional free-form notes.
    pub notes: Option<String>,
    /// Who provided the feedback (e.g. agent id, user name).
    pub caller: Option<String>,
}

/// Version manifest for an evolved skill.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillVersionManifest", get_all, set_all)
)]
pub struct SkillVersionManifest {
    /// Current semantic version.
    pub current_version: String,
    /// History of versions.
    pub history: Vec<SkillVersionEntry>,
}

/// A single version entry in the skill history.
#[derive(Debug, Clone, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillVersionEntry", get_all, set_all)
)]
pub struct SkillVersionEntry {
    /// Version string at this point.
    pub version: String,
    /// When this version was saved.
    pub saved_at: String,
    /// Optional reason / changelog.
    pub reason: Option<String>,
}

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

/// Get user-level accumulated skill search paths from environment variable.
///
/// Reads `DCC_MCP_USER_SKILL_PATHS` (colon-separated on Unix, semicolon on Windows).
/// Only returns paths that exist on the filesystem.
#[must_use]
pub fn get_user_skill_paths_from_env() -> Vec<String> {
    let mut paths = Vec::new();
    if let Ok(value) = env::var(crate::constants::ENV_USER_SKILL_PATHS) {
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
    if let Ok(value) = env::var(crate::constants::ENV_TEAM_SKILL_PATHS) {
        for p in value.split(if cfg!(windows) { ';' } else { ':' }) {
            let p = p.trim();
            if !p.is_empty() && Path::new(p).is_dir() {
                paths.push(p.to_string());
            }
        }
    }
    paths
}

/// Get user-level accumulated skill search paths for a specific app from environment variables.
///
/// Reads **both** paths in priority order (most specific first):
/// 1. `DCC_MCP_USER_{APP}_SKILL_PATHS` — per-app paths
/// 2. `DCC_MCP_USER_SKILL_PATHS` — global fallback
///
/// Paths are deduplicated while preserving order. Only existing directories
/// are returned.
#[must_use]
pub fn get_app_user_skill_paths_from_env(app_name: &str) -> Vec<String> {
    use crate::constants::user_skill_paths_env_key;

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

    let app_key = user_skill_paths_env_key(app_name);
    if let Ok(v) = env::var(&app_key) {
        add(&v);
    }

    if let Ok(v) = env::var(crate::constants::ENV_USER_SKILL_PATHS) {
        add(&v);
    }

    paths
}

/// Get team-level accumulated skill search paths for a specific app from environment variables.
///
/// Reads **both** paths in priority order (most specific first):
/// 1. `DCC_MCP_TEAM_{APP}_SKILL_PATHS` — per-app paths
/// 2. `DCC_MCP_TEAM_SKILL_PATHS` — global fallback
///
/// Paths are deduplicated while preserving order. Only existing directories
/// are returned.
#[must_use]
pub fn get_app_team_skill_paths_from_env(app_name: &str) -> Vec<String> {
    use crate::constants::team_skill_paths_env_key;

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

    let app_key = team_skill_paths_env_key(app_name);
    if let Ok(v) = env::var(&app_key) {
        add(&v);
    }

    if let Ok(v) = env::var(crate::constants::ENV_TEAM_SKILL_PATHS) {
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
    let mut dir = data_dir_path()?.join("skills").join("user");
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
    let mut dir = data_dir_path()?.join("skills").join("team");
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
    let skill_md = src.join(crate::constants::SKILL_METADATA_FILE);
    if !skill_md.is_file() {
        return Err(FilesystemError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!(
                "source directory missing {}: {}",
                crate::constants::SKILL_METADATA_FILE,
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
        archive_skill_version(&dest, None)?;
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

    // Update version manifest
    update_version_manifest(&dest, skill_name)?;

    Ok(dest)
}

fn archive_skill_version(skill_dir: &Path, reason: Option<&str>) -> Result<(), FilesystemError> {
    let versions_dir = skill_dir.join(".versions");
    std::fs::create_dir_all(&versions_dir)?;

    let timestamp = format_system_time_now();
    let archive_dir = versions_dir.join(&timestamp);
    std::fs::create_dir_all(&archive_dir)?;

    for entry in std::fs::read_dir(skill_dir)? {
        let entry = entry?;
        let name = entry.file_name();
        let name_str = name.to_string_lossy();
        if name_str == ".versions" || name_str == ".feedback.jsonl" {
            continue;
        }
        let src_path = entry.path();
        let dest_path = archive_dir.join(&name);
        if src_path.is_dir() {
            copy_dir_recursive(&src_path, &dest_path)?;
        } else {
            std::fs::copy(&src_path, &dest_path)?;
        }
    }

    // Append to manifest history
    let manifest_path = skill_dir.join("version_manifest.json");
    let mut manifest: SkillVersionManifest = if manifest_path.is_file() {
        let content = std::fs::read_to_string(&manifest_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SkillVersionManifest::default()
    };

    manifest.history.push(SkillVersionEntry {
        version: timestamp.clone(),
        saved_at: timestamp.clone(),
        reason: reason.map(|s| s.to_string()),
    });
    manifest.current_version = timestamp;

    let json = serde_json::to_string_pretty(&manifest)?;
    std::fs::write(&manifest_path, json)?;

    Ok(())
}

fn update_version_manifest(skill_dir: &Path, _skill_name: &str) -> Result<(), FilesystemError> {
    let manifest_path = skill_dir.join("version_manifest.json");
    let mut manifest: SkillVersionManifest = if manifest_path.is_file() {
        let content = std::fs::read_to_string(&manifest_path)?;
        serde_json::from_str(&content).unwrap_or_default()
    } else {
        SkillVersionManifest {
            current_version: "1.0.0".to_string(),
            history: Vec::new(),
        }
    };

    if manifest.history.is_empty() {
        let now = format_system_time_now();
        manifest.history.push(SkillVersionEntry {
            version: manifest.current_version.clone(),
            saved_at: now,
            reason: Some("Initial version".to_string()),
        });
        let json = serde_json::to_string_pretty(&manifest)?;
        std::fs::write(&manifest_path, json)?;
    }

    Ok(())
}

fn format_system_time_now() -> String {
    let now = std::time::SystemTime::now();
    let duration = now
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default();
    let secs = duration.as_secs();
    let dt = time_from_secs(secs);
    format!(
        "{:04}{:02}{:02}_{:02}{:02}{:02}",
        dt.0, dt.1, dt.2, dt.3, dt.4, dt.5
    )
}

/// Simple (year, month, day, hour, minute, second) from unix seconds.
fn time_from_secs(secs: u64) -> (u32, u32, u32, u32, u32, u32) {
    let days = secs / 86_400;
    let mut rem = secs % 86_400;
    let hour = (rem / 3_600) as u32;
    rem %= 3_600;
    let minute = (rem / 60) as u32;
    let second = (rem % 60) as u32;

    // Approximate date from days since 1970-01-01 (good enough for versioning)
    let mut year = 1970u32;
    let mut days_left = days as u32;
    loop {
        let days_in_year = if is_leap_year(year) { 366 } else { 365 };
        if days_left < days_in_year {
            break;
        }
        days_left -= days_in_year;
        year += 1;
    }

    let month_days = if is_leap_year(year) {
        [31, 29, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    } else {
        [31, 28, 31, 30, 31, 30, 31, 31, 30, 31, 30, 31]
    };

    let mut month = 1u32;
    for (idx, md) in month_days.iter().enumerate() {
        if days_left < *md {
            month = (idx + 1) as u32;
            break;
        }
        days_left -= *md;
        month = (idx + 2) as u32;
    }
    let day = days_left + 1;

    (year, month, day, hour, minute, second)
}

fn is_leap_year(year: u32) -> bool {
    (year % 4 == 0 && year % 100 != 0) || (year % 400 == 0)
}

// ── Feedback collection ────────────────────────────────────────────────────

/// Record feedback for a skill.
///
/// Appends a JSON line to `<skill_dir>/.feedback.jsonl`.
///
/// # Errors
/// Returns [`FilesystemError`] if the skill directory is invalid or writing fails.
pub fn record_skill_feedback(
    skill_dir: &Path,
    success: bool,
    correction: Option<&str>,
    notes: Option<&str>,
    caller: Option<&str>,
) -> Result<(), FilesystemError> {
    if !skill_dir.is_dir() {
        return Err(FilesystemError::Io(std::io::Error::new(
            std::io::ErrorKind::InvalidInput,
            format!("skill directory not found: {}", skill_dir.display()),
        )));
    }

    let feedback = SkillFeedback {
        timestamp: format_system_time_now(),
        success,
        correction: correction.map(|s| s.to_string()),
        notes: notes.map(|s| s.to_string()),
        caller: caller.map(|s| s.to_string()),
    };

    let feedback_path = skill_dir.join(".feedback.jsonl");
    let line = serde_json::to_string(&feedback)?;
    let mut file = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(&feedback_path)?;
    use std::io::Write;
    writeln!(file, "{line}")?;

    Ok(())
}

/// Read feedback entries for a skill.
///
/// Reads `<skill_dir>/.feedback.jsonl` and returns the most recent `limit` entries.
///
/// # Errors
/// Returns [`FilesystemError`] if reading fails.
pub fn get_skill_feedback(
    skill_dir: &Path,
    limit: Option<usize>,
) -> Result<Vec<SkillFeedback>, FilesystemError> {
    let feedback_path = skill_dir.join(".feedback.jsonl");
    if !feedback_path.is_file() {
        return Ok(Vec::new());
    }

    let content = std::fs::read_to_string(&feedback_path)?;
    let mut entries: Vec<SkillFeedback> = content
        .lines()
        .filter(|l| !l.trim().is_empty())
        .filter_map(|line| serde_json::from_str(line).ok())
        .collect();

    // Return most recent first
    entries.reverse();
    if let Some(n) = limit {
        entries.truncate(n);
    }
    Ok(entries)
}

/// Read version manifest for a skill.
///
/// Reads `<skill_dir>/version_manifest.json`.
///
/// # Errors
/// Returns [`FilesystemError`] if reading fails.
pub fn get_skill_version_manifest(
    skill_dir: &Path,
) -> Result<SkillVersionManifest, FilesystemError> {
    let manifest_path = skill_dir.join("version_manifest.json");
    if !manifest_path.is_file() {
        return Ok(SkillVersionManifest::default());
    }
    let content = std::fs::read_to_string(&manifest_path)?;
    let manifest: SkillVersionManifest = serde_json::from_str(&content).unwrap_or_default();
    Ok(manifest)
}

fn copy_dir_recursive(src: &Path, dst: &Path) -> std::io::Result<()> {
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
    py_dir_binding!(py_get_tools_dir, "get_tools_dir", get_tools_dir, dcc_name: &str);

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

    #[pyfunction]
    #[pyo3(name = "record_skill_feedback")]
    #[pyo3(signature = (skill_dir, success, correction=None, notes=None, caller=None))]
    pub fn py_record_skill_feedback(
        skill_dir: &str,
        success: bool,
        correction: Option<&str>,
        notes: Option<&str>,
        caller: Option<&str>,
    ) -> PyResult<()> {
        record_skill_feedback(Path::new(skill_dir), success, correction, notes, caller)?;
        Ok(())
    }

    #[pyfunction]
    #[pyo3(name = "get_skill_feedback")]
    #[pyo3(signature = (skill_dir, limit=None))]
    pub fn py_get_skill_feedback(
        skill_dir: &str,
        limit: Option<usize>,
    ) -> PyResult<Vec<SkillFeedback>> {
        Ok(get_skill_feedback(Path::new(skill_dir), limit)?)
    }

    #[pyfunction]
    #[pyo3(name = "get_skill_version_manifest")]
    pub fn py_get_skill_version_manifest(skill_dir: &str) -> PyResult<SkillVersionManifest> {
        Ok(get_skill_version_manifest(Path::new(skill_dir))?)
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

    // ── user / team skill paths from env ────────────────────────────────────────

    #[test]
    fn test_get_user_skill_paths_from_env_with_temp_dir() {
        use std::env;
        use std::fs;
        let tmp = env::temp_dir().join("dcc_mcp_test_user_skill_path");
        let _ = fs::create_dir_all(&tmp);
        let tmp_str = tmp.to_string_lossy().to_string();

        unsafe {
            env::set_var(crate::constants::ENV_USER_SKILL_PATHS, &tmp_str);
        }
        let paths = get_user_skill_paths_from_env();
        unsafe {
            env::remove_var(crate::constants::ENV_USER_SKILL_PATHS);
        }
        let _ = fs::remove_dir(&tmp);

        assert!(
            paths
                .iter()
                .any(|p| p.contains("dcc_mcp_test_user_skill_path"))
        );
    }

    #[test]
    fn test_get_team_skill_paths_from_env_with_temp_dir() {
        use std::env;
        use std::fs;
        let tmp = env::temp_dir().join("dcc_mcp_test_team_skill_path");
        let _ = fs::create_dir_all(&tmp);
        let tmp_str = tmp.to_string_lossy().to_string();

        unsafe {
            env::set_var(crate::constants::ENV_TEAM_SKILL_PATHS, &tmp_str);
        }
        let paths = get_team_skill_paths_from_env();
        unsafe {
            env::remove_var(crate::constants::ENV_TEAM_SKILL_PATHS);
        }
        let _ = fs::remove_dir(&tmp);

        assert!(
            paths
                .iter()
                .any(|p| p.contains("dcc_mcp_test_team_skill_path"))
        );
    }

    // ── user / team skills dir ──────────────────────────────────────────────────

    #[test]
    fn test_get_user_skills_dir() {
        let result = get_user_skills_dir(None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("user"));
    }

    #[test]
    fn test_get_team_skills_dir() {
        let result = get_team_skills_dir(None);
        assert!(result.is_ok());
        assert!(result.unwrap().contains("team"));
    }

    #[test]
    fn test_get_user_skills_dir_with_dcc() {
        let result = get_user_skills_dir(Some("maya"));
        assert!(result.is_ok());
        let path = result.unwrap();
        assert!(path.contains("user"));
        assert!(path.contains("maya"));
    }

    // ── copy skill to scope dir ─────────────────────────────────────────────────

    #[test]
    fn test_copy_skill_to_user_dir_ok() {
        use std::env;
        use std::fs;
        let tmp_src = env::temp_dir().join("dcc_mcp_test_copy_skill_src");
        let _ = fs::create_dir_all(tmp_src.join("scripts"));
        let _ = fs::write(tmp_src.join("SKILL.md"), "---\nname: test-skill\n---\n");

        let dest = copy_skill_to_user_dir(&tmp_src, None).unwrap();
        assert!(dest.exists());
        assert!(dest.join("SKILL.md").exists());

        // cleanup
        let _ = fs::remove_dir_all(&tmp_src);
        let _ = fs::remove_dir_all(&dest);
    }

    #[test]
    fn test_copy_skill_to_user_dir_missing_skill_md() {
        use std::env;
        use std::fs;
        let tmp_src = env::temp_dir().join("dcc_mcp_test_copy_skill_bad");
        let _ = fs::create_dir_all(&tmp_src);
        // no SKILL.md

        let result = copy_skill_to_user_dir(&tmp_src, None);
        assert!(result.is_err());

        let _ = fs::remove_dir_all(&tmp_src);
    }
}
