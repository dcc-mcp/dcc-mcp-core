//! SKILL.md loader — parse YAML frontmatter, enumerate scripts, and discover metadata/.
//!
//! The main entry points are:
//!
//! - [`parse_skill_md`]: Load a single skill from a directory.
//! - [`scan_and_load`]: Full pipeline — scan directories, load all skills, resolve dependencies.
//! - [`scan_and_load_lenient`]: Same pipeline but skips skills with missing deps instead of failing.

#[cfg(feature = "python-bindings")]
use pyo3::prelude::*;

use dcc_mcp_models::{SkillGroup, SkillMetadata, ToolDeclaration};
use dcc_mcp_utils::constants::{
    DEPENDS_FILE, SKILL_METADATA_DIR, SKILL_METADATA_FILE, SKILL_SCRIPTS_DIR,
};
use dcc_mcp_utils::filesystem::path_to_string;
use std::path::Path;

/// Namespace prefix for agentskills.io-compliant dcc-mcp-core metadata keys
/// (issue #356). Keys under `metadata.dcc-mcp.*` take priority over the
/// legacy top-level form.
const DCC_MCP_PREFIX: &str = "dcc-mcp.";

/// Top-level YAML keys allowed by the agentskills.io 1.0 spec; any other
/// extension key observed at the frontmatter root is considered legacy
/// (see issue #356).
const AGENTSKILLS_SPEC_KEYS: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
    "allowed_tools",
];

/// Legacy top-level extension keys we still dual-read for backward
/// compatibility. Collected into `SkillMetadata::legacy_extension_fields`
/// so callers can surface a deprecation warning. See issue #356.
const LEGACY_EXTENSION_KEYS: &[&str] = &[
    "dcc",
    "version",
    "tags",
    "search-hint",
    "search_hint",
    "depends",
    "tools",
    "groups",
    "policy",
    "external_deps",
    "external-deps",
    "products",
    "allow_implicit_invocation",
    "allow-implicit-invocation",
    // Issue #342 — per-tool `next-tools` MUST live in the sibling
    // tools.yaml file. A top-level `next-tools:` in SKILL.md is the
    // legacy form and is treated as spec-non-compliant.
    "next-tools",
    "next_tools",
];

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

    // Parse once into a raw YAML value so we can inspect which top-level
    // keys the author declared; this drives the legacy/spec-compliant
    // detection in issue #356 without breaking the existing deserializer.
    let raw_value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(frontmatter) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "Error parsing frontmatter in {}: {}",
                skill_md_path.display(),
                e
            );
            return None;
        }
    };

    let mut meta: SkillMetadata = match serde_yaml_ng::from_value(raw_value.clone()) {
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

    // serde_yaml_ng cannot deserialize directly into `serde_json::Value`
    // for arbitrary YAML mappings — do the conversion manually so callers
    // that rely on `SkillMetadata::metadata` (flat_metadata, openclaw, …)
    // continue to work.
    if let Some(raw_metadata) = raw_value
        .as_mapping()
        .and_then(|m| m.get(serde_yaml_ng::Value::String("metadata".into())))
    {
        if let Some(j) = yaml_to_json(raw_metadata) {
            meta.metadata = j;
        }
    }

    // Apply the agentskills.io-compliant `metadata.dcc-mcp.*` overrides
    // and collect any legacy top-level extension keys that were used.
    let legacy_fields = detect_legacy_extension_fields(&raw_value);
    apply_dcc_mcp_metadata_overrides(skill_dir, &raw_value, &mut meta);
    if !legacy_fields.is_empty() {
        tracing::warn!(
            "skill {name}: legacy top-level field(s) {legacy:?}; use metadata.dcc-mcp.* instead \
             (see docs/guide/skills.md#migrating-pre-015-skillmd)",
            name = meta.name,
            legacy = legacy_fields,
        );
    }
    meta.legacy_extension_fields = legacy_fields;

    // Enumerate scripts
    meta.scripts = enumerate_scripts(skill_dir);
    meta.skill_path = path_to_string(skill_dir);

    // Discover metadata/ directory files
    meta.metadata_files = enumerate_metadata_files(skill_dir);

    // Merge depends from metadata/depends.md if present
    merge_depends_from_metadata(skill_dir, &mut meta);

    Some(meta)
}

// ── Issue #356: agentskills.io-compliant metadata.dcc-mcp.* support ──

/// Collect the names of legacy top-level extension keys that were
/// declared in the raw YAML frontmatter.  Returns an empty vec when the
/// skill already uses the `metadata.dcc-mcp.*` form exclusively.
fn detect_legacy_extension_fields(root: &serde_yaml_ng::Value) -> Vec<String> {
    let Some(map) = root.as_mapping() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    for (key, _) in map.iter() {
        let Some(k) = key.as_str() else { continue };
        if AGENTSKILLS_SPEC_KEYS.contains(&k) {
            continue;
        }
        if LEGACY_EXTENSION_KEYS.contains(&k) {
            let normalized = k.to_string();
            if !out.iter().any(|x: &String| x == &normalized) {
                out.push(normalized);
            }
        }
    }
    out
}

/// Apply `metadata.dcc-mcp.*` overrides onto `meta`.
///
/// Priority: a value present under `metadata.dcc-mcp.<field>` wins over
/// the legacy top-level form.  Missing keys leave the existing value
/// untouched so the legacy path remains functional.  Sibling-file
/// references for `tools` / `groups` are resolved relative to
/// `skill_dir`.
fn apply_dcc_mcp_metadata_overrides(
    skill_dir: &Path,
    raw: &serde_yaml_ng::Value,
    meta: &mut SkillMetadata,
) {
    let overrides = collect_dcc_mcp_overrides(raw);
    if overrides.is_empty() {
        return;
    }

    for (key, value) in overrides {
        match key.as_str() {
            "dcc" => {
                if let Some(s) = value.as_str() {
                    meta.dcc = s.to_string();
                }
            }
            "version" => {
                if let Some(s) = yaml_scalar_as_string(&value) {
                    meta.version = s;
                }
            }
            "tags" => {
                meta.tags = parse_csv_or_list(&value);
            }
            "search-hint" | "search_hint" => {
                if let Some(s) = value.as_str() {
                    meta.search_hint = s.to_string();
                }
            }
            "depends" => {
                meta.depends = parse_csv_or_list(&value);
            }
            "products" => {
                let products = parse_csv_or_list(&value);
                let policy = meta.policy.get_or_insert_with(Default::default);
                policy.products = products;
            }
            "allow-implicit-invocation" | "allow_implicit_invocation" => {
                if let Some(b) = parse_bool_yaml(&value) {
                    let policy = meta.policy.get_or_insert_with(Default::default);
                    policy.allow_implicit_invocation = Some(b);
                }
            }
            "external-deps" | "external_deps" => {
                if let Some(deps) = parse_external_deps_yaml(&value) {
                    meta.external_deps = Some(deps);
                }
            }
            "tools" => {
                if let Some(s) = value.as_str() {
                    if let Some((tools, groups)) = load_sibling_tools_file(skill_dir, s) {
                        meta.tools = tools;
                        if let Some(g) = groups {
                            meta.groups = g;
                        }
                    }
                }
            }
            "groups" => {
                if let Some(s) = value.as_str() {
                    if let Some(groups) = load_sibling_groups_file(skill_dir, s) {
                        meta.groups = groups;
                    }
                }
            }
            _ => {
                tracing::debug!(
                    "skill {}: unknown metadata.dcc-mcp.{} key — ignoring",
                    meta.name,
                    key
                );
            }
        }
    }
}

/// Extract `metadata.dcc-mcp.*` overrides from the raw YAML frontmatter.
///
/// The prefix strip is applied to keys; returns pairs of
/// `(field_suffix, raw_value)` so callers can interpret each override in
/// the correct type.
fn collect_dcc_mcp_overrides(raw: &serde_yaml_ng::Value) -> Vec<(String, serde_yaml_ng::Value)> {
    let mut out = Vec::new();
    let Some(map) = raw.as_mapping() else {
        return out;
    };
    let Some(meta_node) = map.get(serde_yaml_ng::Value::String("metadata".into())) else {
        return out;
    };
    let Some(meta_map) = meta_node.as_mapping() else {
        return out;
    };
    for (k, v) in meta_map.iter() {
        let Some(ks) = k.as_str() else { continue };
        if let Some(rest) = ks.strip_prefix(DCC_MCP_PREFIX) {
            out.push((rest.to_string(), v.clone()));
        }
    }
    out
}

/// Accept either a comma-separated string (`"a, b, c"`) or a YAML list.
/// Empty / invalid inputs yield an empty vec.
fn parse_csv_or_list(v: &serde_yaml_ng::Value) -> Vec<String> {
    if let Some(s) = v.as_str() {
        return s
            .split(',')
            .map(|t| t.trim().to_string())
            .filter(|t| !t.is_empty())
            .collect();
    }
    if let Some(seq) = v.as_sequence() {
        return seq
            .iter()
            .filter_map(|x| x.as_str().map(String::from))
            .collect();
    }
    Vec::new()
}

/// Parse a boolean from a native YAML bool or a `"true"`/`"false"`
/// string (case-insensitive).  Everything else → `None`.
fn parse_bool_yaml(v: &serde_yaml_ng::Value) -> Option<bool> {
    if let Some(b) = v.as_bool() {
        return Some(b);
    }
    if let Some(s) = v.as_str() {
        match s.trim().to_ascii_lowercase().as_str() {
            "true" | "yes" | "1" => return Some(true),
            "false" | "no" | "0" => return Some(false),
            _ => {}
        }
    }
    None
}

/// Coerce a YAML scalar to its string representation. Handles both
/// `"1.0.0"` and unquoted `1.0.0` (YAML may parse the latter as a
/// float / string depending on lexer quirks).
fn yaml_scalar_as_string(v: &serde_yaml_ng::Value) -> Option<String> {
    if let Some(s) = v.as_str() {
        return Some(s.to_string());
    }
    if let Some(i) = v.as_i64() {
        return Some(i.to_string());
    }
    if let Some(f) = v.as_f64() {
        return Some(f.to_string());
    }
    None
}

/// Parse a JSON-encoded string (per issue #356) or an inline YAML object
/// into a [`SkillDependencies`].  Returns `None` when the value is
/// unusable.
fn parse_external_deps_yaml(v: &serde_yaml_ng::Value) -> Option<dcc_mcp_models::SkillDependencies> {
    if let Some(s) = v.as_str() {
        return serde_json::from_str(s).ok();
    }
    serde_yaml_ng::from_value(v.clone()).ok()
}

/// Recursively convert a `serde_yaml_ng::Value` into a
/// `serde_json::Value`. Non-string mapping keys are coerced with
/// `to_string()` so the result always round-trips through a JSON
/// object.
fn yaml_to_json(v: &serde_yaml_ng::Value) -> Option<serde_json::Value> {
    use serde_json::Value as J;
    Some(match v {
        serde_yaml_ng::Value::Null => J::Null,
        serde_yaml_ng::Value::Bool(b) => J::Bool(*b),
        serde_yaml_ng::Value::Number(n) => {
            if let Some(i) = n.as_i64() {
                J::Number(i.into())
            } else if let Some(u) = n.as_u64() {
                J::Number(u.into())
            } else if let Some(f) = n.as_f64() {
                serde_json::Number::from_f64(f)
                    .map(J::Number)
                    .unwrap_or(J::Null)
            } else {
                J::Null
            }
        }
        serde_yaml_ng::Value::String(s) => J::String(s.clone()),
        serde_yaml_ng::Value::Sequence(seq) => {
            J::Array(seq.iter().filter_map(yaml_to_json).collect())
        }
        serde_yaml_ng::Value::Mapping(map) => {
            let mut obj = serde_json::Map::new();
            for (k, val) in map.iter() {
                let key = match k {
                    serde_yaml_ng::Value::String(s) => s.clone(),
                    other => {
                        // Best-effort: stringify non-string keys.
                        serde_yaml_ng::to_string(other)
                            .unwrap_or_default()
                            .trim()
                            .to_string()
                    }
                };
                if let Some(jv) = yaml_to_json(val) {
                    obj.insert(key, jv);
                }
            }
            J::Object(obj)
        }
        serde_yaml_ng::Value::Tagged(t) => return yaml_to_json(&t.value),
    })
}

/// Load a sibling YAML file referenced by `metadata.dcc-mcp.tools`.
///
/// The file must be a YAML mapping with a top-level `tools:` key and an
/// optional `groups:` key, e.g.:
///
/// ```yaml
/// tools:
///   - name: create_sphere
///     description: ...
/// groups:
///   - name: advanced
///     default-active: false
/// ```
fn load_sibling_tools_file(
    skill_dir: &Path,
    rel: &str,
) -> Option<(Vec<ToolDeclaration>, Option<Vec<SkillGroup>>)> {
    if !has_yaml_extension(rel) {
        tracing::warn!(
            "metadata.dcc-mcp.tools references {rel:?} which is not a .yaml/.yml file; ignoring"
        );
        return None;
    }
    let path = skill_dir.join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("failed to read sibling tools file {}: {e}", path.display());
            return None;
        }
    };

    #[derive(serde::Deserialize, Default)]
    struct Sidecar {
        #[serde(default)]
        tools: Option<serde_yaml_ng::Value>,
        #[serde(default)]
        groups: Option<Vec<SkillGroup>>,
    }

    let side: Sidecar = match serde_yaml_ng::from_str(&text) {
        Ok(s) => s,
        Err(e) => {
            tracing::warn!("failed to parse sibling tools file {}: {e}", path.display());
            return None;
        }
    };

    let tools = match side.tools {
        Some(v) => deserialize_tools_value(v)?,
        None => Vec::new(),
    };
    Some((tools, side.groups))
}

/// Load a sibling YAML file referenced by `metadata.dcc-mcp.groups`.
///
/// The file must be a YAML mapping whose top-level `groups:` key is a
/// list of [`SkillGroup`] declarations.
fn load_sibling_groups_file(skill_dir: &Path, rel: &str) -> Option<Vec<SkillGroup>> {
    if !has_yaml_extension(rel) {
        tracing::warn!(
            "metadata.dcc-mcp.groups references {rel:?} which is not a .yaml/.yml file; ignoring"
        );
        return None;
    }
    let path = skill_dir.join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!("failed to read sibling groups file {}: {e}", path.display());
            return None;
        }
    };

    #[derive(serde::Deserialize, Default)]
    struct Sidecar {
        #[serde(default)]
        groups: Option<Vec<SkillGroup>>,
    }

    match serde_yaml_ng::from_str::<Sidecar>(&text) {
        Ok(s) => s.groups,
        Err(e) => {
            tracing::warn!(
                "failed to parse sibling groups file {}: {e}",
                path.display()
            );
            None
        }
    }
}

fn has_yaml_extension(path: &str) -> bool {
    let lower = path.to_ascii_lowercase();
    lower.ends_with(".yaml") || lower.ends_with(".yml")
}

/// Deserialize a YAML value into the same `Vec<ToolDeclaration>` shape
/// accepted by the main SKILL.md `tools:` key (plain names or full
/// declaration objects).
fn deserialize_tools_value(value: serde_yaml_ng::Value) -> Option<Vec<ToolDeclaration>> {
    let Some(seq) = value.as_sequence() else {
        tracing::warn!("sibling tools file: `tools:` must be a list");
        return None;
    };
    let mut out = Vec::with_capacity(seq.len());
    for item in seq {
        match item {
            serde_yaml_ng::Value::String(s) => out.push(ToolDeclaration {
                name: s.clone(),
                ..Default::default()
            }),
            serde_yaml_ng::Value::Mapping(_) => {
                match serde_yaml_ng::from_value::<ToolDeclaration>(item.clone()) {
                    Ok(t) => out.push(t),
                    Err(e) => {
                        tracing::warn!("sibling tools file: invalid tool entry: {e}");
                        return None;
                    }
                }
            }
            _ => {
                tracing::warn!("sibling tools file: each tool must be a string or mapping");
                return None;
            }
        }
    }
    Some(out)
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
///
/// Accepts either a skill directory path or a direct path to a `SKILL.md` file.
/// If a file path is given, the parent directory is used automatically.
///
/// Returns `None` if the directory contains no valid `SKILL.md`.
/// Raises `FileNotFoundError` if the path does not exist at all.
#[cfg(feature = "python-bindings")]
#[pyfunction]
#[pyo3(name = "parse_skill_md")]
pub fn py_parse_skill_md(skill_dir: &str) -> pyo3::PyResult<Option<SkillMetadata>> {
    let raw = Path::new(skill_dir);

    // Resolve: if the user passed a path to `SKILL.md` (or any file), use the parent dir.
    let dir = if raw.is_file() {
        raw.parent()
            .ok_or_else(|| {
                pyo3::exceptions::PyValueError::new_err(format!(
                    "parse_skill_md: cannot determine parent directory of file: {skill_dir}"
                ))
            })?
            .to_owned()
    } else if raw.is_dir() {
        raw.to_owned()
    } else {
        // Path doesn't exist at all — raise a clear error instead of silently returning None.
        return Err(pyo3::exceptions::PyFileNotFoundError::new_err(format!(
            "parse_skill_md: path does not exist: {skill_dir}"
        )));
    };

    Ok(parse_skill_md(&dir))
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
