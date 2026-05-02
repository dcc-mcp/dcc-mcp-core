//! Skill version manifest, archival snapshots and ISO-like timestamping.
//!
//! Skill-specific filesystem layer; the underlying [`FilesystemError`] is
//! re-used from [`dcc-mcp-paths`].

use crate::paths::copy_dir_recursive;
use dcc_mcp_paths::FilesystemError;
use std::path::Path;

/// Version manifest for an evolved skill.
#[derive(Debug, Clone, Default, serde::Serialize, serde::Deserialize)]
#[cfg_attr(
    feature = "python-bindings",
    pyo3::pyclass(name = "SkillVersionManifest", get_all, set_all, skip_from_py_object)
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
    pyo3::pyclass(name = "SkillVersionEntry", get_all, set_all, from_py_object)
)]
pub struct SkillVersionEntry {
    /// Version string at this point.
    pub version: String,
    /// When this version was saved.
    pub saved_at: String,
    /// Optional reason / changelog.
    pub reason: Option<String>,
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

/// Snapshot the current contents of `skill_dir` into `.versions/<timestamp>/`
/// and append an entry to the version manifest.
pub(crate) fn archive_skill_version(
    skill_dir: &Path,
    reason: Option<&str>,
) -> Result<(), FilesystemError> {
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

/// Initialize the version manifest for a freshly copied skill if it doesn't exist.
pub(crate) fn update_version_manifest(
    skill_dir: &Path,
    _skill_name: &str,
) -> Result<(), FilesystemError> {
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

pub(crate) fn format_system_time_now() -> String {
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
    (year.is_multiple_of(4) && !year.is_multiple_of(100)) || year.is_multiple_of(400)
}

// PyO3 bindings live in `crate::python::versioning`.
#[cfg(feature = "python-bindings")]
pub use crate::python::versioning::py_get_skill_version_manifest;
