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

// ── Accumulated skills (user / team) ────────────────────────────────────────

/// Scan user-level accumulated skill paths from environment variables.
pub fn scan_and_load_user(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let user_paths = if let Some(dcc) = dcc_name {
        dcc_mcp_utils::filesystem::get_app_user_skill_paths_from_env(dcc)
    } else {
        dcc_mcp_utils::filesystem::get_user_skill_paths_from_env()
    };
    let mut all_paths = user_paths;
    if let Some(extra) = extra_paths {
        all_paths.extend(extra.iter().cloned());
    }
    scan_and_load(
        if all_paths.is_empty() {
            None
        } else {
            Some(&all_paths)
        },
        dcc_name,
    )
}

/// Scan team-level accumulated skill paths from environment variables.
pub fn scan_and_load_team(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let team_paths = if let Some(dcc) = dcc_name {
        dcc_mcp_utils::filesystem::get_app_team_skill_paths_from_env(dcc)
    } else {
        dcc_mcp_utils::filesystem::get_team_skill_paths_from_env()
    };
    let mut all_paths = team_paths;
    if let Some(extra) = extra_paths {
        all_paths.extend(extra.iter().cloned());
    }
    scan_and_load(
        if all_paths.is_empty() {
            None
        } else {
            Some(&all_paths)
        },
        dcc_name,
    )
}

/// Lenient variant of [`scan_and_load_user`].
pub fn scan_and_load_user_lenient(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let user_paths = if let Some(dcc) = dcc_name {
        dcc_mcp_utils::filesystem::get_app_user_skill_paths_from_env(dcc)
    } else {
        dcc_mcp_utils::filesystem::get_user_skill_paths_from_env()
    };
    let mut all_paths = user_paths;
    if let Some(extra) = extra_paths {
        all_paths.extend(extra.iter().cloned());
    }
    scan_and_load_lenient(
        if all_paths.is_empty() {
            None
        } else {
            Some(&all_paths)
        },
        dcc_name,
    )
}

/// Lenient variant of [`scan_and_load_team`].
pub fn scan_and_load_team_lenient(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let team_paths = if let Some(dcc) = dcc_name {
        dcc_mcp_utils::filesystem::get_app_team_skill_paths_from_env(dcc)
    } else {
        dcc_mcp_utils::filesystem::get_team_skill_paths_from_env()
    };
    let mut all_paths = team_paths;
    if let Some(extra) = extra_paths {
        all_paths.extend(extra.iter().cloned());
    }
    scan_and_load_lenient(
        if all_paths.is_empty() {
            None
        } else {
            Some(&all_paths)
        },
        dcc_name,
    )
}
