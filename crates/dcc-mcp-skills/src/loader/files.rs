use std::path::Path;

use crate::constants::{
    DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_SCRIPTS_DIR, is_supported_extension,
    is_valid_skill_name,
};
use dcc_mcp_models::SkillMetadata;
use dcc_mcp_paths::path_to_string;

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
                if let Some(ext) = path.extension().and_then(|ext| ext.to_str())
                    && filter(ext)
                {
                    files.push(path_to_string(&path));
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

/// Extract a dependency name from one line of `metadata/depends.md`.
///
/// A dependency is a Markdown list item (`- name`) or a bare name, and it must
/// additionally look like a skill name: kebab-case, at most 64 characters, no
/// leading/trailing or consecutive hyphens (see [`crate::constants::is_valid_skill_name`]).
///
/// Skill names are slugs, so any line that fails that shape is prose — an
/// un-commented heading, a stray prose word such as `Optional`, or a list item
/// with interior whitespace. Treating such lines as names turned one
/// descriptive sentence into a phantom dependency that failed resolution for
/// every skill that declared it, and a name that is not a valid slug could
/// never have resolved in the first place.
fn parse_depends_line(line: &str) -> Option<String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('#') {
        return None;
    }
    let name = trimmed
        .strip_prefix("- ")
        .or_else(|| trimmed.strip_prefix("* "))
        .unwrap_or(trimmed)
        .trim();
    if !is_valid_skill_name(name) {
        tracing::debug!("Ignoring non-slug line in {DEPENDS_FILE}: {name:?}");
        return None;
    }
    Some(name.to_string())
}

/// Parse metadata/depends.md and merge dependency names into meta.depends.
///
/// Blank lines, `#` headings and prose are skipped; see `parse_depends_line`.
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
        let dep_name = match parse_depends_line(line) {
            Some(dep_name) => dep_name,
            None => continue,
        };
        if !meta.depends.iter().any(|dep| dep == &dep_name) {
            meta.depends.push(dep_name);
        }
    }
}
