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
fn load_all_skills(dirs: &[String]) -> (Vec<SkillMetadata>, Vec<String>) {
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
fn extract_frontmatter(content: &str) -> Option<&str> {
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
fn enumerate_scripts(skill_dir: &Path) -> Vec<String> {
    enumerate_files_by_ext(&skill_dir.join(SKILL_SCRIPTS_DIR), |ext| {
        dcc_mcp_utils::constants::is_supported_extension(ext)
    })
}

/// Enumerate .md files in the metadata/ subdirectory.
fn enumerate_metadata_files(skill_dir: &Path) -> Vec<String> {
    enumerate_files_by_ext(&skill_dir.join(SKILL_METADATA_DIR), |ext| {
        ext.eq_ignore_ascii_case("md")
    })
}

/// Parse metadata/depends.md and merge dependency names into meta.depends.
///
/// depends.md format: one dependency name per line (ignoring blank lines and # comments).
fn merge_depends_from_metadata(skill_dir: &Path, meta: &mut SkillMetadata) {
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
mod tests {
    use super::*;

    // ── extract_frontmatter ──

    mod test_extract_frontmatter {
        use super::*;

        #[test]
        fn valid_frontmatter() {
            let content = "---\nname: test\ndescription: hello\n---\n# Body";
            let fm = extract_frontmatter(content).unwrap();
            assert!(fm.contains("name: test"));
            assert!(fm.contains("description: hello"));
        }

        #[test]
        fn no_frontmatter() {
            assert!(extract_frontmatter("no frontmatter").is_none());
        }

        #[test]
        fn empty_frontmatter() {
            let content = "---\n---\n# Body";
            let fm = extract_frontmatter(content).unwrap();
            assert!(fm.is_empty());
        }

        #[test]
        fn frontmatter_with_lists() {
            let content = "---\nname: test\ntags:\n  - geometry\n  - creation\n---\nBody";
            let fm = extract_frontmatter(content).unwrap();
            assert!(fm.contains("tags:"));
            assert!(fm.contains("- geometry"));
        }

        #[test]
        fn no_closing_delimiter() {
            let content = "---\nname: test\nno closing delimiter";
            assert!(extract_frontmatter(content).is_none());
        }
    }

    // ── enumerate helpers (using tempfile) ──

    mod test_enumerate {
        use super::*;

        #[test]
        fn enumerate_scripts_discovers_supported_files() {
            let tmp = tempfile::tempdir().unwrap();
            let scripts_dir = tmp.path().join(SKILL_SCRIPTS_DIR);
            std::fs::create_dir_all(&scripts_dir).unwrap();

            std::fs::write(scripts_dir.join("setup.py"), "# python").unwrap();
            std::fs::write(scripts_dir.join("run.mel"), "// mel").unwrap();
            std::fs::write(scripts_dir.join("notes.txt"), "not a script").unwrap();

            let result = enumerate_scripts(tmp.path());
            // .py and .mel are supported; .txt is not
            assert!(
                result.iter().any(|p| p.ends_with("setup.py")),
                "Expected .py file in {result:?}"
            );
            assert!(
                result.iter().any(|p| p.ends_with("run.mel")),
                "Expected .mel file in {result:?}"
            );
            assert!(
                !result.iter().any(|p| p.ends_with("notes.txt")),
                "Should not include .txt in {result:?}"
            );
        }

        #[test]
        fn enumerate_scripts_empty_when_no_dir() {
            let tmp = tempfile::tempdir().unwrap();
            // No scripts/ directory exists
            let result = enumerate_scripts(tmp.path());
            assert!(result.is_empty());
        }

        #[test]
        fn enumerate_metadata_files_discovers_md() {
            let tmp = tempfile::tempdir().unwrap();
            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();

            std::fs::write(meta_dir.join("help.md"), "# Help").unwrap();
            std::fs::write(meta_dir.join("install.md"), "# Install").unwrap();
            std::fs::write(meta_dir.join("data.json"), "{}").unwrap();

            let result = enumerate_metadata_files(tmp.path());
            assert_eq!(result.len(), 2, "Should find exactly 2 .md files");
            assert!(result.iter().any(|p| p.ends_with("help.md")));
            assert!(result.iter().any(|p| p.ends_with("install.md")));
            assert!(!result.iter().any(|p| p.ends_with("data.json")));
        }
    }

    // ── merge_depends_from_metadata ──

    mod test_merge_depends {
        use super::*;

        fn make_skill_with_deps(deps: &[&str]) -> SkillMetadata {
            SkillMetadata {
                depends: deps.iter().map(|s| s.to_string()).collect(),
                ..Default::default()
            }
        }

        #[test]
        fn merge_plain_text_format() {
            let tmp = tempfile::tempdir().unwrap();
            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();
            std::fs::write(meta_dir.join(DEPENDS_FILE), "dep-a\ndep-b\n").unwrap();

            let mut meta = make_skill_with_deps(&[]);
            merge_depends_from_metadata(tmp.path(), &mut meta);

            assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
        }

        #[test]
        fn merge_yaml_list_format() {
            let tmp = tempfile::tempdir().unwrap();
            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();
            std::fs::write(meta_dir.join(DEPENDS_FILE), "- alpha\n- beta\n").unwrap();

            let mut meta = make_skill_with_deps(&[]);
            merge_depends_from_metadata(tmp.path(), &mut meta);

            assert_eq!(meta.depends, vec!["alpha", "beta"]);
        }

        #[test]
        fn merge_skips_comments_and_blanks() {
            let tmp = tempfile::tempdir().unwrap();
            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();
            std::fs::write(
                meta_dir.join(DEPENDS_FILE),
                "# Comment\n\ndep-a\n\n# Another comment\ndep-b\n",
            )
            .unwrap();

            let mut meta = make_skill_with_deps(&[]);
            merge_depends_from_metadata(tmp.path(), &mut meta);

            assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
        }

        #[test]
        fn merge_deduplicates_with_existing() {
            let tmp = tempfile::tempdir().unwrap();
            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();
            std::fs::write(meta_dir.join(DEPENDS_FILE), "dep-a\ndep-b\ndep-a\n").unwrap();

            let mut meta = make_skill_with_deps(&["dep-a"]);
            merge_depends_from_metadata(tmp.path(), &mut meta);

            // dep-a should not be duplicated
            assert_eq!(meta.depends, vec!["dep-a", "dep-b"]);
        }

        #[test]
        fn merge_noop_when_no_file() {
            let tmp = tempfile::tempdir().unwrap();
            // No metadata/ directory
            let mut meta = make_skill_with_deps(&["existing"]);
            merge_depends_from_metadata(tmp.path(), &mut meta);
            assert_eq!(meta.depends, vec!["existing"]);
        }
    }

    // ── parse_skill_md (full integration) ──

    mod test_parse_skill_md {
        use super::*;

        /// Helper to create a minimal SKILL.md content.
        fn skill_md(name: &str, dcc: &str, deps: &[&str]) -> String {
            let deps_str = if deps.is_empty() {
                String::new()
            } else {
                format!(
                    "\ndepends:\n{}",
                    deps.iter()
                        .map(|d| format!("  - {d}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            format!("---\nname: {name}\ndcc: {dcc}{deps_str}\n---\n# {name}\n\nDescription text.")
        }

        #[test]
        fn parse_valid_skill() {
            let tmp = tempfile::tempdir().unwrap();
            let content = skill_md("my-skill", "maya", &[]);
            std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

            let meta = parse_skill_md(tmp.path()).unwrap();
            assert_eq!(meta.name, "my-skill");
            assert_eq!(meta.dcc, "maya");
            assert!(meta.depends.is_empty());
            assert!(!meta.skill_path.is_empty());
        }

        #[test]
        fn parse_skill_with_depends() {
            let tmp = tempfile::tempdir().unwrap();
            let content = skill_md("pipeline", "houdini", &["geometry", "usd-tools"]);
            std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

            let meta = parse_skill_md(tmp.path()).unwrap();
            assert_eq!(meta.name, "pipeline");
            assert_eq!(meta.depends, vec!["geometry", "usd-tools"]);
        }

        #[test]
        fn parse_skill_with_scripts() {
            let tmp = tempfile::tempdir().unwrap();
            let content = skill_md("scripted", "blender", &[]);
            std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

            let scripts_dir = tmp.path().join(SKILL_SCRIPTS_DIR);
            std::fs::create_dir_all(&scripts_dir).unwrap();
            std::fs::write(scripts_dir.join("run.py"), "print('hello')").unwrap();

            let meta = parse_skill_md(tmp.path()).unwrap();
            assert_eq!(meta.scripts.len(), 1);
            assert!(meta.scripts[0].ends_with("run.py"));
        }

        #[test]
        fn parse_skill_with_metadata_depends() {
            let tmp = tempfile::tempdir().unwrap();
            let content = skill_md("composite", "maya", &["frontmatter-dep"]);
            std::fs::write(tmp.path().join(SKILL_METADATA_FILE), &content).unwrap();

            let meta_dir = tmp.path().join(SKILL_METADATA_DIR);
            std::fs::create_dir_all(&meta_dir).unwrap();
            std::fs::write(meta_dir.join(DEPENDS_FILE), "file-dep\n").unwrap();

            let meta = parse_skill_md(tmp.path()).unwrap();
            assert!(meta.depends.contains(&"frontmatter-dep".to_string()));
            assert!(meta.depends.contains(&"file-dep".to_string()));
        }

        #[test]
        fn parse_skill_fallback_name_from_dir() {
            let tmp = tempfile::tempdir().unwrap();
            // Frontmatter with empty name => should use directory name
            std::fs::write(
                tmp.path().join(SKILL_METADATA_FILE),
                "---\nname: \"\"\ndcc: python\n---\n# Unnamed",
            )
            .unwrap();

            let meta = parse_skill_md(tmp.path()).unwrap();
            // Name should be the directory name (tempdir's last component)
            assert!(!meta.name.is_empty());
        }

        #[test]
        fn parse_returns_none_for_missing_skill_md() {
            let tmp = tempfile::tempdir().unwrap();
            // No SKILL.md file
            assert!(parse_skill_md(tmp.path()).is_none());
        }

        #[test]
        fn parse_returns_none_for_invalid_yaml() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::write(
                tmp.path().join(SKILL_METADATA_FILE),
                "---\n: invalid: yaml: [broken\n---\n",
            )
            .unwrap();
            assert!(parse_skill_md(tmp.path()).is_none());
        }

        #[test]
        fn parse_returns_none_for_no_frontmatter() {
            let tmp = tempfile::tempdir().unwrap();
            std::fs::write(
                tmp.path().join(SKILL_METADATA_FILE),
                "Just plain markdown without frontmatter.",
            )
            .unwrap();
            assert!(parse_skill_md(tmp.path()).is_none());
        }
    }

    // ── scan_and_load pipeline ──

    mod test_scan_and_load {
        use super::*;

        fn create_skill(base: &Path, name: &str, dcc: &str, deps: &[&str]) {
            let skill_dir = base.join(name);
            std::fs::create_dir_all(&skill_dir).unwrap();

            let deps_str = if deps.is_empty() {
                String::new()
            } else {
                format!(
                    "\ndepends:\n{}",
                    deps.iter()
                        .map(|d| format!("  - {d}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            let content =
                format!("---\nname: {name}\ndcc: {dcc}{deps_str}\n---\n# {name}\n\nBody.");
            std::fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
        }

        #[test]
        fn load_empty_paths() {
            let result = scan_and_load(Some(&["/nonexistent-path".to_string()]), None).unwrap();
            assert!(result.skills.is_empty());
            assert!(result.skipped.is_empty());
        }

        #[test]
        fn load_single_skill() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "basic", "python", &[]);

            let result =
                scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
            assert_eq!(result.skills.len(), 1);
            assert_eq!(result.skills[0].name, "basic");
        }

        #[test]
        fn load_with_dependency_order() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "base", "python", &[]);
            create_skill(tmp.path(), "middle", "python", &["base"]);
            create_skill(tmp.path(), "top", "python", &["middle"]);

            let result =
                scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
            assert_eq!(result.skills.len(), 3);

            let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
            let base_pos = names.iter().position(|&n| n == "base").unwrap();
            let middle_pos = names.iter().position(|&n| n == "middle").unwrap();
            let top_pos = names.iter().position(|&n| n == "top").unwrap();
            assert!(base_pos < middle_pos, "base must come before middle");
            assert!(middle_pos < top_pos, "middle must come before top");
        }

        #[test]
        fn load_fails_on_missing_dependency() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "broken", "python", &["nonexistent"]);

            let err =
                scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
            assert!(matches!(err, ResolveError::MissingDependency { .. }));
        }

        #[test]
        fn load_fails_on_cycle() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "a", "python", &["b"]);
            create_skill(tmp.path(), "b", "python", &["a"]);

            let err =
                scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap_err();
            assert!(matches!(err, ResolveError::CyclicDependency { .. }));
        }

        #[test]
        fn load_tracks_skipped_dirs() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "good", "python", &[]);

            // Create a directory without a valid SKILL.md
            let bad_dir = tmp.path().join("bad");
            std::fs::create_dir_all(&bad_dir).unwrap();
            std::fs::write(bad_dir.join(SKILL_METADATA_FILE), "no frontmatter at all").unwrap();

            let result =
                scan_and_load(Some(&[tmp.path().to_string_lossy().to_string()]), None).unwrap();
            assert_eq!(result.skills.len(), 1);
            assert_eq!(result.skills[0].name, "good");
            assert_eq!(result.skipped.len(), 1);
        }
    }

    // ── scan_and_load_lenient ──

    mod test_scan_and_load_lenient {
        use super::*;

        fn create_skill(base: &Path, name: &str, deps: &[&str]) {
            let skill_dir = base.join(name);
            std::fs::create_dir_all(&skill_dir).unwrap();

            let deps_str = if deps.is_empty() {
                String::new()
            } else {
                format!(
                    "\ndepends:\n{}",
                    deps.iter()
                        .map(|d| format!("  - {d}"))
                        .collect::<Vec<_>>()
                        .join("\n")
                )
            };
            let content =
                format!("---\nname: {name}\ndcc: python{deps_str}\n---\n# {name}\n\nBody.");
            std::fs::write(skill_dir.join(SKILL_METADATA_FILE), &content).unwrap();
        }

        #[test]
        fn lenient_skips_missing_deps() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "good", &[]);
            create_skill(tmp.path(), "broken", &["nonexistent"]);

            let result =
                scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None)
                    .unwrap();
            assert_eq!(result.skills.len(), 1);
            assert_eq!(result.skills[0].name, "good");
            // broken should be in skipped
            assert!(!result.skipped.is_empty());
        }

        #[test]
        fn lenient_still_fails_on_cycle() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "a", &["b"]);
            create_skill(tmp.path(), "b", &["a"]);

            let err =
                scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None)
                    .unwrap_err();
            assert!(matches!(err, ResolveError::CyclicDependency { .. }));
        }

        #[test]
        fn lenient_preserves_valid_skills() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "base", &[]);
            create_skill(tmp.path(), "child", &["base"]);
            create_skill(tmp.path(), "orphan", &["missing-dep"]);

            let result =
                scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None)
                    .unwrap();
            let names: Vec<&str> = result.skills.iter().map(|s| s.name.as_str()).collect();
            assert!(names.contains(&"base"));
            assert!(names.contains(&"child"));
            assert!(!names.contains(&"orphan"));
        }

        #[test]
        fn lenient_empty_when_all_valid() {
            let tmp = tempfile::tempdir().unwrap();
            create_skill(tmp.path(), "a", &[]);
            create_skill(tmp.path(), "b", &["a"]);

            let result =
                scan_and_load_lenient(Some(&[tmp.path().to_string_lossy().to_string()]), None)
                    .unwrap();
            assert_eq!(result.skills.len(), 2);
            // No parse-failures, no dependency-failures
            assert!(result.skipped.is_empty());
        }
    }

    // ── load_all_skills helper ──

    mod test_load_all_skills {
        use super::*;

        #[test]
        fn load_mixed_valid_and_invalid() {
            let tmp = tempfile::tempdir().unwrap();

            // Valid skill
            let valid_dir = tmp.path().join("valid");
            std::fs::create_dir_all(&valid_dir).unwrap();
            std::fs::write(
                valid_dir.join(SKILL_METADATA_FILE),
                "---\nname: valid\n---\n# Valid",
            )
            .unwrap();

            // Invalid skill (no frontmatter)
            let invalid_dir = tmp.path().join("invalid");
            std::fs::create_dir_all(&invalid_dir).unwrap();
            std::fs::write(
                invalid_dir.join(SKILL_METADATA_FILE),
                "plain text, no frontmatter",
            )
            .unwrap();

            let dirs = vec![
                valid_dir.to_string_lossy().to_string(),
                invalid_dir.to_string_lossy().to_string(),
            ];
            let (skills, skipped) = load_all_skills(&dirs);
            assert_eq!(skills.len(), 1);
            assert_eq!(skills[0].name, "valid");
            assert_eq!(skipped.len(), 1);
        }

        #[test]
        fn load_nonexistent_dirs() {
            let dirs = vec!["/definitely/does/not/exist".to_string()];
            let (skills, skipped) = load_all_skills(&dirs);
            assert!(skills.is_empty());
            assert_eq!(skipped.len(), 1);
        }
    }
}
