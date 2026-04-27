use std::path::Path;

use crate::constants::{
    DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_SCRIPTS_DIR, is_supported_extension,
};
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_utils::filesystem::path_to_string;

/// Enumerate files in a directory matching a filter predicate on the file extension.
fn enumerate_files_by_ext(dir: &Path, filter: impl Fn(&str) -> bool) -> Vec<String> {
    if !dir.is_dir() {
        return vec![];
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|entry| match entry {
            Ok(entry) => Some(entry),
            Err(err) => {
                tracing::warn!("Skipping unreadable entry in {}: {err}", dir.display());
                None
            }
        }) {
            let file_type = match entry.file_type() {
                Ok(file_type) => file_type,
                Err(err) => {
                    tracing::debug!(
                        "Cannot read file type for {}: {err}",
                        entry.path().display()
                    );
                    continue;
                }
            };
            if file_type.is_file() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|ext| ext.to_str()) {
                    if filter(ext) {
                        files.push(path_to_string(&path));
                    }
                }
            }
        }
    }
    files.sort();
    files
}

/// Enumerate script files in the scripts/ subdirectory.
pub(crate) fn enumerate_scripts(skill_dir: &Path) -> Vec<String> {
    enumerate_files_by_ext(&skill_dir.join(SKILL_SCRIPTS_DIR), is_supported_extension)
}

/// Enumerate .md files in the metadata/ subdirectory.
pub(crate) fn enumerate_metadata_files(skill_dir: &Path) -> Vec<String> {
    enumerate_files_by_ext(&skill_dir.join(SKILL_METADATA_DIR), |ext| {
        ext.eq_ignore_ascii_case("md")
    })
}

/// Parse metadata/depends.md and merge dependency names into meta.depends.
pub(crate) fn merge_depends_from_metadata(skill_dir: &Path, meta: &mut SkillMetadata) {
    let depends_path = skill_dir.join(SKILL_METADATA_DIR).join(DEPENDS_FILE);
    if !depends_path.is_file() {
        return;
    }

    let content = match std::fs::read_to_string(&depends_path) {
        Ok(content) => content,
        Err(err) => {
            tracing::warn!("Error reading {}: {}", depends_path.display(), err);
            return;
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        let dep_name = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim();
        if !dep_name.is_empty() && !meta.depends.iter().any(|dep| dep == dep_name) {
            meta.depends.push(dep_name.to_string());
        }
    }
}
