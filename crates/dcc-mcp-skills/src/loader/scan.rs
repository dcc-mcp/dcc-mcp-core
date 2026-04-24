use std::path::Path;

use dcc_mcp_models::SkillMetadata;

use crate::resolver::{self, ResolveError};
use crate::scanner::SkillScanner;

use super::parse_skill_md;

/// Result of a full scan-and-load pipeline.
#[derive(Debug, Clone)]
pub struct LoadResult {
    /// Skills in dependency-resolved order (dependencies come first).
    pub skills: Vec<SkillMetadata>,
    /// Directories that were scanned but failed to load (parse errors, missing SKILL.md, etc.).
    pub skipped: Vec<String>,
}

/// Full pipeline: scan directories for skills, load metadata, and resolve dependencies.
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
pub fn scan_and_load_lenient(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs = scanner.scan(extra_paths, dcc_name, false);

    let (skills, mut skipped) = load_all_skills(&dirs);
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
            .filter(|skill| {
                if bad_skills.contains(&skill.name) {
                    skipped.push(skill.skill_path.clone());
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
