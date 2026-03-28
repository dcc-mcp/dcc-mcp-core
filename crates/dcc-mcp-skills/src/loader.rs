//! SKILL.md loader — parse YAML frontmatter and enumerate scripts.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_models::SkillMetadata;
use dcc_mcp_utils::constants::{SKILL_METADATA_FILE, SKILL_SCRIPTS_DIR, SUPPORTED_SCRIPT_EXTENSIONS};
use std::path::{Path, PathBuf};

/// Parse a SKILL.md file from a skill directory.
pub fn parse_skill_md(skill_dir: &Path) -> Option<SkillMetadata> {
    let skill_md_path = skill_dir.join(SKILL_METADATA_FILE);
    if !skill_md_path.is_file() {
        tracing::warn!("SKILL.md not found at: {}", skill_md_path.display());
        return None;
    }

    let content = match std::fs::read_to_string(&skill_md_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Error reading {}: {}", skill_md_path.display(), e);
            return None;
        }
    };

    // Extract YAML frontmatter between --- delimiters
    let frontmatter = extract_frontmatter(&content)?;

    // Parse YAML
    let mut meta: SkillMetadata = match serde_yaml::from_str(&frontmatter) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!("Error parsing frontmatter in {}: {}", skill_md_path.display(), e);
            return None;
        }
    };

    // Ensure name exists
    if meta.name.is_empty() {
        meta.name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().to_string())
            .unwrap_or_default();
    }

    // Enumerate scripts
    meta.scripts = enumerate_scripts(skill_dir);
    meta.skill_path = skill_dir.to_string_lossy().to_string();

    Some(meta)
}

/// Extract YAML frontmatter from content.
fn extract_frontmatter(content: &str) -> Option<String> {
    if !content.starts_with("---") {
        return None;
    }
    let after_first = &content[3..];
    let end = after_first.find("\n---")?;
    Some(after_first[..end].trim().to_string())
}

/// Enumerate script files in the scripts/ subdirectory.
fn enumerate_scripts(skill_dir: &Path) -> Vec<String> {
    let scripts_dir = skill_dir.join(SKILL_SCRIPTS_DIR);
    if !scripts_dir.is_dir() {
        return vec![];
    }

    let mut scripts = Vec::new();
    if let Ok(entries) = std::fs::read_dir(&scripts_dir) {
        for entry in entries.flatten() {
            let path = entry.path();
            if path.is_file() {
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
                    let ext_lower = format!(".{}", ext.to_lowercase());
                    if SUPPORTED_SCRIPT_EXTENSIONS.contains_key(ext_lower.as_str()) {
                        scripts.push(path.to_string_lossy().to_string());
                    }
                }
            }
        }
    }
    scripts.sort();
    scripts
}

/// Python wrapper for parse_skill_md.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "parse_skill_md")]
pub fn py_parse_skill_md(skill_dir: &str) -> Option<SkillMetadata> {
    parse_skill_md(Path::new(skill_dir))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_frontmatter() {
        let content = "---\nname: test\ndescription: hello\n---\n# Body";
        let fm = extract_frontmatter(content).unwrap();
        assert!(fm.contains("name: test"));
    }

    #[test]
    fn test_extract_frontmatter_none() {
        assert!(extract_frontmatter("no frontmatter").is_none());
    }
}
