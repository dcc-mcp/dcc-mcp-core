use std::collections::HashMap;
use std::path::Path;

use dcc_mcp_models::SkillMetadata;

use crate::catalog::scoring::SkillPathSource;
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

/// Result of a source-aware scan-and-load pipeline.
///
/// Identical to [`LoadResult`] but pairs each loaded skill with the
/// [`SkillPathSource`] of the search root it was found under (issue
/// #1403). The catalog uses this to apply a small rank penalty so user-
/// curated locations outrank bundled starter material for neutral queries.
#[derive(Debug, Clone)]
pub struct LoadResultWithSources {
    /// Skills in dependency-resolved order, paired with their source.
    pub skills: Vec<(SkillMetadata, SkillPathSource)>,
    /// Directories that were scanned but failed to load.
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

/// Strict pipeline: identical to [`scan_and_load`] but rejects the load
/// when any scanned directory failed to produce a loadable skill.
///
/// Issue maya#138 — operators repeatedly hit the failure mode where a
/// bad `SKILL.md` (missing frontmatter, malformed YAML, wrong filename)
/// caused the scanner to silently elide the directory at `tracing::debug`,
/// leaving a hard-to-diagnose "tool went missing" symptom at run-time.
/// `scan_and_load_strict` surfaces those skipped directories as a
/// [`ResolveError::SkippedDirectories`] so embedders can fail start-up
/// loudly instead.  Dependency resolution still runs first to keep error
/// ordering deterministic (cycle / missing-dep errors win over skipped
/// directories).
pub fn scan_and_load_strict(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs = scanner.scan(extra_paths, dcc_name, false);
    let missing_skill_md = SkillScanner::scan_explicit_directories_missing_skill_md(extra_paths);

    let (skills, mut skipped) = load_all_skills(&dirs);
    let resolved = resolver::resolve_dependencies(&skills)?;

    for dir in missing_skill_md {
        if !skipped.contains(&dir) {
            tracing::warn!(
                directory = %dir,
                "Strict skill scan found a directory without SKILL.md; rejecting discovery"
            );
            skipped.push(dir);
        }
    }

    if !skipped.is_empty() {
        return Err(ResolveError::SkippedDirectories {
            directories: skipped,
        });
    }

    Ok(LoadResult {
        skills: resolved.ordered,
        skipped,
    })
}

/// Lenient pipeline: scan, load, and resolve dependencies while keeping
/// skills with missing soft dependencies discoverable.
pub fn scan_and_load_lenient(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResult, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs = scanner.scan(extra_paths, dcc_name, false);

    let (skills, skipped) = load_all_skills(&dirs);
    let errors = resolver::validate_dependencies(&skills);
    if !errors.is_empty() {
        for err in &errors {
            if let ResolveError::MissingDependency { skill, dependency } = err {
                tracing::warn!(
                    "Skill '{skill}' depends on '{dependency}' which is not available yet; \
                     keeping it discoverable as a pending dependency."
                );
            }
        }
        let resolved = resolve_dependencies_soft(&skills)?;
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

/// Source-aware variant of [`scan_and_load_lenient`] (issue #1403).
///
/// Performs the same scan / parse / resolve pipeline, but tracks which
/// search-root each skill was discovered under so the catalog can apply
/// the path-source rank penalty. The dependency-resolved skill order is
/// preserved; the per-skill source is matched back by `skill_path`.
pub fn scan_and_load_lenient_with_sources(
    extra_paths: Option<&[String]>,
    dcc_name: Option<&str>,
) -> Result<LoadResultWithSources, ResolveError> {
    let mut scanner = SkillScanner::new();
    let dirs_with_sources = scanner.scan_with_sources(extra_paths, dcc_name, false);
    let source_by_dir: HashMap<String, SkillPathSource> = dirs_with_sources
        .iter()
        .map(|(d, s)| (d.clone(), *s))
        .collect();
    let dirs: Vec<String> = dirs_with_sources.into_iter().map(|(d, _)| d).collect();

    let (skills, skipped) = load_all_skills(&dirs);
    let errors = resolver::validate_dependencies(&skills);
    let ordered = if errors.is_empty() {
        resolver::resolve_dependencies(&skills)?.ordered
    } else {
        for err in &errors {
            if let ResolveError::MissingDependency { skill, dependency } = err {
                tracing::warn!(
                    "Skill '{skill}' depends on '{dependency}' which is not available yet; \
                     keeping it discoverable as a pending dependency."
                );
            }
        }
        resolve_dependencies_soft(&skills)?.ordered
    };

    let paired = ordered
        .into_iter()
        .map(|meta| {
            let src = source_by_dir
                .get(&meta.skill_path)
                .copied()
                .unwrap_or_default();
            (meta, src)
        })
        .collect();
    Ok(LoadResultWithSources {
        skills: paired,
        skipped,
    })
}

fn resolve_dependencies_soft(
    skills: &[SkillMetadata],
) -> Result<resolver::ResolvedSkills, ResolveError> {
    let names: std::collections::HashSet<&str> = skills.iter().map(|s| s.name.as_str()).collect();
    let orderable: Vec<SkillMetadata> = skills
        .iter()
        .map(|skill| {
            let mut clone = skill.clone();
            clone.depends.retain(|dep| names.contains(dep.as_str()));
            clone
        })
        .collect();
    let resolved = resolver::resolve_dependencies(&orderable)?;
    let originals: std::collections::HashMap<&str, &SkillMetadata> = skills
        .iter()
        .map(|skill| (skill.name.as_str(), skill))
        .collect();
    let ordered = resolved
        .ordered
        .into_iter()
        .filter_map(|skill| originals.get(skill.name.as_str()).map(|s| (*s).clone()))
        .collect();
    Ok(resolver::ResolvedSkills { ordered })
}

/// Load all skill metadata from a list of directories.
///
/// Issue maya#138: skipped directories are now reported at `warn` level
/// (was `debug`) so operators can immediately see why a skill they
/// expected went missing.  The per-failure root cause — missing
/// `SKILL.md`, malformed YAML, missing required field — is logged at
/// `warn` from inside [`parse_skill_md`]; the line emitted here is the
/// summary marker that ties those low-level diagnostics back to the
/// scan pipeline.
pub(crate) fn load_all_skills(dirs: &[String]) -> (Vec<SkillMetadata>, Vec<String>) {
    let mut skills = Vec::new();
    let mut skipped = Vec::new();

    for dir_str in dirs {
        let dir = Path::new(dir_str);
        match parse_skill_md(dir) {
            Some(meta) => skills.push(meta),
            None => {
                tracing::warn!(
                    directory = %dir_str,
                    "Skipping skill directory: SKILL.md missing or failed validation \
                     (see preceding parse warnings for the specific cause). \
                     Use scan_and_load_strict() to fail-fast on such directories."
                );
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
        crate::paths::get_app_user_skill_paths_from_env(dcc)
    } else {
        crate::paths::get_user_skill_paths_from_env()
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
        crate::paths::get_app_team_skill_paths_from_env(dcc)
    } else {
        crate::paths::get_team_skill_paths_from_env()
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
        crate::paths::get_app_user_skill_paths_from_env(dcc)
    } else {
        crate::paths::get_user_skill_paths_from_env()
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
        crate::paths::get_app_team_skill_paths_from_env(dcc)
    } else {
        crate::paths::get_team_skill_paths_from_env()
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
