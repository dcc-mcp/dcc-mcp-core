//! SKILL.md loader — parse YAML frontmatter, enumerate scripts, and discover metadata/.
//!
//! The main entry points are:
//!
//! - [`parse_skill_md`]: Load a single skill from a directory.
//! - [`scan_and_load`]: Full pipeline — scan directories, load all skills, resolve dependencies.
//! - [`scan_and_load_lenient`]: Same pipeline but skips skills with missing deps instead of failing.

// PyO3 bindings live in `crate::python::loader`.

use crate::constants::SKILL_METADATA_FILE;
use dcc_mcp_models::{SkillGroup, SkillMetadata, ToolDeclaration};
use dcc_mcp_paths::path_to_string;
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

mod files;
mod scan;

pub(crate) use files::{enumerate_metadata_files, enumerate_scripts, merge_depends_from_metadata};
#[cfg(test)]
pub(crate) use scan::load_all_skills;
pub use scan::{
    LoadResult, scan_and_load, scan_and_load_lenient, scan_and_load_strict, scan_and_load_team,
    scan_and_load_team_lenient, scan_and_load_user, scan_and_load_user_lenient,
};

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
        && let Some(j) = yaml_to_json(raw_metadata)
    {
        meta.metadata = j;
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
                if let Some(s) = value.as_str()
                    && let Some((tools, groups)) = load_sibling_tools_file(skill_dir, s)
                {
                    meta.tools = tools;
                    if let Some(g) = groups {
                        meta.groups = g;
                    }
                }
            }
            "groups" => {
                if let Some(s) = value.as_str()
                    && let Some(groups) = load_sibling_groups_file(skill_dir, s)
                {
                    meta.groups = groups;
                }
            }
            "prompts" => {
                // Issues #351, #355 — sibling-file reference for the MCP
                // prompts primitive. Parsing is deferred; we just record
                // the path (relative to skill root) so the MCP server can
                // load it lazily on `prompts/list` / `prompts/get`.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.prompts_file = Some(s.to_string());
                }
            }
            "layer" => {
                // Architectural layer for skill routing and search partitioning.
                // Valid values: "infrastructure", "domain", "example".
                // See skills/README.md#skill-layering and AGENTS.md.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.layer = Some(s.to_string());
                }
            }
            "recipes" => {
                // Sibling-file reference for pre-composed parameter templates
                // (issue #466). Parsing is deferred; store the path for lazy loading.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.recipes_file = Some(s.to_string());
                }
            }
            "introspection" => {
                // Sibling-file reference for capability-probe / version-check
                // metadata (issue #466). Parsing is deferred; store for lazy loading.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.introspection_file = Some(s.to_string());
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
        // Flat form: `metadata: { "dcc-mcp.dcc": "maya", ... }` — pre-0.15
        // shorthand preserved for back-compat.
        if let Some(rest) = ks.strip_prefix(DCC_MCP_PREFIX) {
            out.push((rest.to_string(), v.clone()));
            continue;
        }
        // Nested form: `metadata: { dcc-mcp: { dcc: maya, ... } }` —
        // canonical agentskills.io-compliant shape (issue #356) and the
        // shape produced by the sibling-file migration tool.
        if ks == "dcc-mcp"
            && let Some(inner) = v.as_mapping()
        {
            for (ik, iv) in inner.iter() {
                let Some(iks) = ik.as_str() else { continue };
                out.push((iks.to_string(), iv.clone()));
            }
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

// ── Python bindings live in `crate::python::loader` ──

#[cfg(feature = "python-bindings")]
pub use crate::python::loader::{
    py_parse_skill_md, py_scan_and_load, py_scan_and_load_lenient, py_scan_and_load_strict,
    py_scan_and_load_team, py_scan_and_load_team_lenient, py_scan_and_load_user,
    py_scan_and_load_user_lenient,
};

// ── Tests ──

#[cfg(test)]
mod tests;
