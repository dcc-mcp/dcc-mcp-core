//! SKILL.md loader — parse YAML frontmatter, enumerate scripts, and discover metadata/.
//!
//! The main entry points are:
//!
//! - [`parse_skill_md`]: Load a single skill from a directory.
//! - [`scan_and_load`]: Full pipeline — scan directories, load all skills, resolve dependencies.
//! - [`scan_and_load_lenient`]: Same pipeline but skips skills with missing deps instead of failing.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_models::SkillMetadata;
use dcc_mcp_utils::constants::{
    DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_METADATA_FILE, SKILL_SCRIPTS_DIR,
};
use dcc_mcp_utils::filesystem::path_to_string;
use std::path::Path;

use crate::resolver::{self, ResolveError};
use crate::scanner::SkillScanner;

// ── Single skill loading ──

/// Parse a SKILL.md file from a skill directory.
#[must_use]
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
    let mut meta: SkillMetadata = match serde_yaml_ng::from_str(frontmatter) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Error parsing frontmatter in {}: {}",
                skill_md_path.display(),
                e
            );
            return None;
        }
    };

    // Ensure name exists
    if meta.name.is_empty() {
        meta.name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
    }

    // Enumerate scripts
    meta.scripts = enumerate_scripts(skill_dir);
    meta.skill_path = path_to_string(skill_dir);

    // Discover metadata/ directory files
    meta.metadata_files = enumerate_metadata_files(skill_dir);

    // Merge depends from metadata/depends.md if present
    merge_depends_from_metadata(skill_dir, &mut meta);

    Some(meta)
}

// ── Full pipeline: scan → load → resolve ──

/// Result of a full scan-and-load pipeline.
#[derive(Debug, Clone)]
pub struct LoadResult {
    /// Skills in dependency-resolved order (dependencies come first).
    pub skills: Vec<SkillMetadata>,
    /// Directories that were scanned but failed to load (parse errors, missing SKILL.md, etc.).
    pub skipped: Vec<String>,
}

/// Full pipeline: scan directories for skills, load metadata, and resolve dependencies.
///
/// 1. Scan `extra_paths` + env + platform paths for skill directories.
/// 2. Parse each discovered directory's SKILL.md into [`SkillMetadata`].
/// 3. Topologically sort by declared dependencies (strict — errors on missing deps or cycles).
///
/// # Errors
///
/// Returns [`ResolveError`] if any loaded skill declares a dependency that was not found
/// among the loaded set, or if a dependency cycle is detected.
pub fn scan_and_load(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs = scanner.scan(extra_paths, dcc_name, false);

    let (skills, skipped) = load_all_skills(&dirs);

    let resolved = resolver::resolve_dependencies(&skills)?;

    Ok(LoadResult {
        skills: resolved.ordered,
        skipped,
    })
}

/// Lenient pipeline: scan, load, and resolve dependencies but skip unresolvable skills.
///
/// Unlike [`scan_and_load`], this variant:
/// - Validates dependencies and logs warnings for missing ones.
/// - Filters out skills with missing dependencies.
/// - Attempts to resolve the remaining subset.
/// - Only fails on cycles (which indicate a structural problem).
///
/// This is useful in production where some skills may reference optional dependencies
/// that are not installed.
///
/// # Errors
///
/// Returns [`ResolveError::CyclicDependency`] if the resolvable subset still has cycles.
pub fn scan_and_load_lenient(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs = scanner.scan(extra_paths, dcc_name, false);

    let (skills, mut skipped) = load_all_skills(&dirs);

    // Validate and filter out skills with missing dependencies
    let errors = resolver::validate_dependencies(&skills);
    if !errors.is_empty() {
        let mut bad_skills = std::collections::HashSet::new();
        for err in &errors {
            if let ResolveError::MissingDependency { skill, dependency } = err {
                tracing::warn!(
                    "Skill '{skill}' depends on '{dependency}' which is not available; skipping."
                );
                bad_skills.insert(skill.clone());
            }
        }

        let filtered: Vec<SkillMetadata> = skills
            .into_iter()
            .filter(|s| {
                if bad_skills.contains(&s.name) {
                    skipped.push(s.skill_path.clone());
                    false
                } else {
                    true
                }
            })
            .collect();

        let resolved = resolver::resolve_dependencies(&filtered)?;
        return Ok(LoadResult {
            skills: resolved.ordered,
            skipped,
        });
    }

    let resolved = resolver::resolve_dependencies(&skills)?;
    Ok(LoadResult {
        skills: resolved.ordered,
        skipped,
    })
}

/// Load all skill metadata from a list of directories.
///
/// Returns (successfully loaded skills, list of directories that failed to load).
pub(crate) fn load_all_skills(dirs: &[String]) -> (Vec<SkillMetadata>, Vec<String>) {
    let mut skills = Vec::new();
    let mut skipped = Vec::new();

    for dir_str in dirs {
        let dir = Path::new(dir_str);
        match parse_skill_md(dir) {
            Some(meta) => skills.push(meta),
            None => {
                tracing::debug!("Skipping directory (failed to parse): {dir_str}");
                skipped.push(dir_str.clone());
            }
        }
    }

    (skills, skipped)
}

// ── Private helpers ──

/// Extract YAML frontmatter from content as a borrowed slice.
pub(crate) fn extract_frontmatter(content: &str) -> Option<&str> {
    const DELIMITER: &str = "---";
    if !content.starts_with(DELIMITER) {
        return None;
    }
    let after_first = &content[DELIMITER.len()..];
    let end = after_first.find("\n---")?;
    Some(after_first[..end].trim())
}

/// Enumerate files in a directory matching a filter predicate on the file extension.
fn enumerate_files_by_ext(dir: &Path, filter: impl Fn(&str) -> bool) -> Vec<String> {
    if !dir.is_dir() {
        return vec![];
    }

    let mut files = Vec::new();
    if let Ok(entries) = std::fs::read_dir(dir) {
        for entry in entries.filter_map(|e| match e {
            Ok(entry) => Some(entry),
            Err(err) => {
                tracing::warn!("Skipping unreadable entry in {}: {err}", dir.display());
                None
            }
        }) {
            let ft = match entry.file_type() {
                Ok(ft) => ft,
                Err(err) => {
                    tracing::debug!(
                        "Cannot read file type for {}: {err}",
                        entry.path().display()
                    );
                    continue;
                }
            };
            if ft.is_file() {
                let path = entry.path();
                if let Some(ext) = path.extension().and_then(|e| e.to_str()) {
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
    enumerate_files_by_ext(&skill_dir.join(SKILL_SCRIPTS_DIR), |ext| {
        dcc_mcp_utils::constants::is_supported_extension(ext)
    })
}

/// Enumerate .md files in the metadata/ subdirectory.
pub(crate) fn enumerate_metadata_files(skill_dir: &Path) -> Vec<String> {
    enumerate_files_by_ext(&skill_dir.join(SKILL_METADATA_DIR), |ext| {
        ext.eq_ignore_ascii_case("md")
    })
}

/// Parse metadata/depends.md and merge dependency names into meta.depends.
///
/// depends.md format: one dependency name per line (ignoring blank lines and # comments).
pub(crate) fn merge_depends_from_metadata(skill_dir: &Path, meta: &mut SkillMetadata) {
    let depends_path = skill_dir.join(SKILL_METADATA_DIR).join(DEPENDS_FILE);
    if !depends_path.is_file() {
        return;
    }

    let content = match std::fs::read_to_string(&depends_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Error reading {}: {}", depends_path.display(), e);
            return;
        }
    };

    for line in content.lines() {
        let trimmed = line.trim();
        // Skip blank lines and comments
        if trimmed.is_empty() || trimmed.starts_with('#') {
            continue;
        }
        // Strip leading "- " for YAML-style lists
        let dep_name = trimmed.strip_prefix("- ").unwrap_or(trimmed).trim();
        if !dep_name.is_empty() && !meta.depends.iter().any(|d| d == dep_name) {
            meta.depends.push(dep_name.to_string());
        }
    }
}

// ── Python bindings ──

/// Python wrapper for parse_skill_md.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "parse_skill_md")]
pub fn py_parse_skill_md(skill_dir: &str) -> Option<SkillMetadata> {
    parse_skill_md(Path::new(skill_dir))
}

/// Python wrapper for scan_and_load (strict mode).
///
/// Returns a tuple of (ordered_skills, skipped_dirs).
/// Raises ValueError on missing dependencies or cycles.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "scan_and_load")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> pyo3::PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

/// Python wrapper for scan_and_load_lenient.
///
/// Returns a tuple of (ordered_skills, skipped_dirs).
/// Skills with missing dependencies are skipped instead of raising errors.
/// Only raises ValueError on cyclic dependencies.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "scan_and_load_lenient")]
#[pyo3(signature = (extra_paths=None, dcc_name=None))]
pub fn py_scan_and_load_lenient(
    extra_paths: Option<Vec<String>>,
    dcc_name: Option<&str>,
) -> pyo3::PyResult<(Vec<SkillMetadata>, Vec<String>)> {
    let result = scan_and_load_lenient(extra_paths.as_deref(), dcc_name)
        .map_err(|e| pyo3::exceptions::PyValueError::new_err(e.to_string()))?;
    Ok((result.skills, result.skipped))
}

// ── Tests ──

#[cfg(test)]
mod tests;
