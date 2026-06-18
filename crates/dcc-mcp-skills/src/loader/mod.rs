//! SKILL.md loader — parse YAML frontmatter, enumerate scripts, and discover metadata/.
//!
//! The main entry points are:
//!
//! - [`parse_skill_md`]: Load a single skill from a directory.
//! - [`scan_and_load`]: Full pipeline — scan directories, load all skills, resolve dependencies.
//! - [`scan_and_load_lenient`]: Same pipeline but keeps skills with missing soft deps discoverable.

// PyO3 bindings live in `crate::python::loader`.

use crate::constants::SKILL_METADATA_FILE;
use dcc_mcp_models::{SkillGroup, SkillMetadata, ToolDeclaration};
use dcc_mcp_paths::path_to_string;
use std::path::Path;

/// Top-level YAML keys allowed by the agentskills.io 1.0 spec. Any other
/// key at the frontmatter root causes [`parse_skill_md`] to reject the
/// skill. All dcc-mcp-core extensions must be expressed under
/// `metadata.dcc-mcp.*` (see issue #356).
const AGENTSKILLS_SPEC_KEYS: &[&str] = &[
    "name",
    "description",
    "license",
    "compatibility",
    "metadata",
    "allowed-tools",
    "allowed_tools",
];

/// Diagnostic captured when a skill directory is scanned but cannot be loaded.
///
/// The payload intentionally carries only a directory basename, not the
/// absolute path. Full paths remain available in local logs and in the legacy
/// `LoadResult.skipped` vector for callers that already opted into local
/// filesystem details.
#[derive(Debug, Clone, PartialEq, Eq, serde::Serialize, serde::Deserialize)]
pub struct SkippedSkillDiagnostic {
    /// Best-effort skill name, read from frontmatter when possible and falling
    /// back to the directory name otherwise.
    pub skill_name: String,
    /// Directory basename, safe to surface through MCP discovery.
    pub directory_name: String,
    /// Stable machine-readable reason code.
    pub reason_code: String,
    /// Human-readable reason with no absolute path.
    pub message: String,
    /// Actionable migration or repair hint.
    pub suggested_fix: String,
    /// Best-effort DCC filter metadata from frontmatter when it can be
    /// parsed safely.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dcc: Option<String>,
    /// Best-effort tag filter metadata from frontmatter when it can be
    /// parsed safely.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub tags: Vec<String>,
    /// Catalog scope that found the skipped directory, when known.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub scope: Option<String>,
    /// Non-spec frontmatter keys that caused rejection, when applicable.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub offending_keys: Vec<String>,
}

impl SkippedSkillDiagnostic {
    fn new(
        skill_dir: &Path,
        skill_name: Option<String>,
        reason_code: impl Into<String>,
        message: impl Into<String>,
        suggested_fix: impl Into<String>,
    ) -> Self {
        let directory_name = skill_dir
            .file_name()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();
        let skill_name = skill_name
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| directory_name.clone());
        Self {
            skill_name,
            directory_name,
            reason_code: reason_code.into(),
            message: message.into(),
            suggested_fix: suggested_fix.into(),
            dcc: None,
            tags: Vec::new(),
            scope: None,
            offending_keys: Vec::new(),
        }
    }

    pub fn with_scope(mut self, scope: impl Into<String>) -> Self {
        self.scope = Some(scope.into());
        self
    }

    fn with_filter_metadata(mut self, dcc: Option<String>, tags: Vec<String>) -> Self {
        self.dcc = dcc.filter(|value| !value.trim().is_empty());
        self.tags = tags;
        self
    }

    fn with_offending_keys(mut self, offending_keys: Vec<String>) -> Self {
        self.offending_keys = offending_keys;
        self
    }

    /// Return true when this diagnostic should be surfaced for a discovery
    /// query. Matching is deliberately simple and local: agents need "why did
    /// maya-mgear disappear?", not a second BM25 index.
    pub fn matches_query(&self, query: Option<&str>) -> bool {
        let q = query.map(str::trim).unwrap_or_default();
        if q.is_empty() {
            return true;
        }
        let q = q.to_ascii_lowercase();
        let haystack = format!(
            "{} {} {} {} {} {}",
            self.skill_name,
            self.directory_name,
            self.reason_code,
            self.message,
            self.suggested_fix,
            self.offending_keys.join(" ")
        )
        .to_ascii_lowercase();
        haystack.contains(&q)
    }
}

mod files;
mod scan;

pub(crate) use files::{enumerate_metadata_files, enumerate_scripts, merge_depends_from_metadata};
#[cfg(test)]
pub(crate) use scan::load_all_skills;
pub use scan::{
    LoadResult, LoadResultWithSources, scan_and_load, scan_and_load_lenient,
    scan_and_load_lenient_with_sources, scan_and_load_strict, scan_and_load_team,
    scan_and_load_team_lenient, scan_and_load_user, scan_and_load_user_lenient,
};

// ── Single skill loading ──

/// Parse a SKILL.md file from a skill directory.
#[must_use]
pub fn parse_skill_md(skill_dir: &Path) -> Option<SkillMetadata> {
    parse_skill_md_with_diagnostic(skill_dir).ok()
}

/// Parse a SKILL.md file from a skill directory, returning a structured
/// skipped diagnostic when the loader rejects it.
pub fn parse_skill_md_with_diagnostic(
    skill_dir: &Path,
) -> Result<SkillMetadata, Box<SkippedSkillDiagnostic>> {
    let skill_md_path = skill_dir.join(SKILL_METADATA_FILE);
    if !skill_md_path.is_file() {
        tracing::warn!("SKILL.md not found at: {}", skill_md_path.display());
        return Err(Box::new(SkippedSkillDiagnostic::new(
            skill_dir,
            None,
            "missing_skill_md",
            "SKILL.md not found",
            "Create a SKILL.md file with agentskills.io frontmatter.",
        )));
    }

    let content = match std::fs::read_to_string(&skill_md_path) {
        Ok(c) => c,
        Err(e) => {
            tracing::warn!("Error reading {}: {}", skill_md_path.display(), e);
            return Err(Box::new(SkippedSkillDiagnostic::new(
                skill_dir,
                None,
                "read_error",
                format!("Cannot read SKILL.md: {e}"),
                "Fix file permissions or encoding, then rescan the skill directory.",
            )));
        }
    };

    // Extract YAML frontmatter between --- delimiters
    let frontmatter = extract_frontmatter(&content).ok_or_else(|| {
        Box::new(SkippedSkillDiagnostic::new(
            skill_dir,
            None,
            "missing_frontmatter",
            "SKILL.md missing YAML frontmatter",
            "Start SKILL.md with a YAML frontmatter block delimited by ---.",
        ))
    })?;

    // Parse once into a raw YAML value so we can validate top-level keys
    // before handing off to serde. All dcc-mcp-core extensions must live
    // under `metadata.dcc-mcp.*` (issue #356); any legacy top-level
    // extension key causes the skill to be rejected.
    let raw_value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(frontmatter) {
        Ok(v) => v,
        Err(e) => {
            tracing::warn!(
                "Error parsing frontmatter in {}: {}",
                skill_md_path.display(),
                e
            );
            return Err(Box::new(SkippedSkillDiagnostic::new(
                skill_dir,
                None,
                "invalid_frontmatter_yaml",
                format!("Invalid YAML frontmatter: {e}"),
                "Fix the YAML frontmatter syntax, then rescan the skill directory.",
            )));
        }
    };

    // Reject any top-level key that is not part of the agentskills.io
    // 1.0 spec. This replaces the pre-0.15 dual-read path that silently
    // accepted legacy top-level extension keys.
    if let Some(map) = raw_value.as_mapping() {
        let mut offending: Vec<String> = map
            .iter()
            .filter_map(|(k, _)| k.as_str())
            .filter(|k| !AGENTSKILLS_SPEC_KEYS.contains(k))
            .map(str::to_string)
            .collect();
        if !offending.is_empty() {
            offending.sort_unstable();
            offending.dedup();
            let suggested_fix = non_spec_top_level_suggestion(&offending);
            tracing::error!(
                "skill at {}: non-spec top-level key(s) {:?}; move them under metadata.dcc-mcp.* \
                 (see docs/guide/skills.md#migrating-pre-015-skillmd)",
                skill_md_path.display(),
                offending,
            );
            return Err(Box::new(
                SkippedSkillDiagnostic::new(
                    skill_dir,
                    raw_skill_name(&raw_value, skill_dir),
                    "non_spec_top_level_keys",
                    format!(
                        "SKILL.md uses non-spec top-level key(s) {:?}; dcc-mcp-core extensions must live under metadata.dcc-mcp.*.",
                        offending
                    ),
                    suggested_fix,
                )
                .with_frontmatter_filter_metadata(&raw_value)
                .with_offending_keys(offending),
            ));
        }
    }

    let mut meta: SkillMetadata = match serde_yaml_ng::from_value(raw_value.clone()) {
        Ok(m) => m,
        Err(e) => {
            tracing::warn!(
                "Error parsing frontmatter in {}: {}",
                skill_md_path.display(),
                e
            );
            return Err(Box::new(SkippedSkillDiagnostic::new(
                skill_dir,
                raw_skill_name(&raw_value, skill_dir),
                "invalid_frontmatter_shape",
                format!("Cannot deserialize frontmatter into SkillMetadata: {e}"),
                "Keep top-level fields to name, description, license, compatibility, metadata, and allowed-tools.",
            )
            .with_frontmatter_filter_metadata(&raw_value)));
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
    //
    // SKILL.md files declare dcc-mcp-core extensions under the nested
    // `metadata: { dcc-mcp: { key: value } }` shape (issue #356). Downstream
    // callers look them up with flat keys like `metadata["dcc-mcp.workflows"]`
    // / `metadata["dcc-mcp.layer"]` — we preserve that wire contract by
    // flattening the nested sub-map into the top-level metadata JSON before
    // handing it off.
    if let Some(raw_metadata) = raw_value
        .as_mapping()
        .and_then(|m| m.get(serde_yaml_ng::Value::String("metadata".into())))
        && let Some(mut j) = yaml_to_json(raw_metadata)
    {
        if let Some(obj) = j.as_object_mut()
            && let Some(inner) = obj.remove("dcc-mcp")
            && let Some(inner_map) = inner.as_object()
        {
            for (k, v) in inner_map {
                obj.insert(format!("dcc-mcp.{k}"), v.clone());
            }
        }
        meta.metadata = j;
    }

    // Apply the agentskills.io-compliant `metadata.dcc-mcp.*` overrides.
    apply_dcc_mcp_metadata_overrides(skill_dir, &raw_value, &mut meta);

    // Enumerate scripts
    meta.scripts = enumerate_scripts(skill_dir);
    meta.skill_path = path_to_string(skill_dir);

    // Discover metadata/ directory files
    meta.metadata_files = enumerate_metadata_files(skill_dir);

    // Merge depends from metadata/depends.md if present
    merge_depends_from_metadata(skill_dir, &mut meta);

    Ok(meta)
}

/// Re-run the loader in diagnostic mode for a directory that was skipped by a
/// scan. If the directory now parses successfully, return a generic stale
/// diagnostic so callers can refresh their catalog state.
#[must_use]
pub fn diagnose_skipped_skill_dir(skill_dir: &Path) -> SkippedSkillDiagnostic {
    match parse_skill_md_with_diagnostic(skill_dir) {
        Ok(meta) => SkippedSkillDiagnostic::new(
            skill_dir,
            Some(meta.name.clone()),
            "stale_skip_record",
            "Skill now parses successfully but the catalog still has a skipped record.",
            "Rediscover skills to refresh the catalog state.",
        )
        .with_filter_metadata((!meta.dcc.is_empty()).then_some(meta.dcc), meta.tags),
        Err(diagnostic) => *diagnostic,
    }
}

impl SkippedSkillDiagnostic {
    fn with_frontmatter_filter_metadata(self, raw: &serde_yaml_ng::Value) -> Self {
        let (dcc, tags) = frontmatter_filter_metadata(raw);
        self.with_filter_metadata(dcc, tags)
    }
}

fn raw_skill_name(raw: &serde_yaml_ng::Value, skill_dir: &Path) -> Option<String> {
    raw.as_mapping()
        .and_then(|map| map.get(serde_yaml_ng::Value::String("name".to_string())))
        .and_then(|value| value.as_str())
        .map(str::to_string)
        .or_else(|| {
            skill_dir
                .file_name()
                .map(|name| name.to_string_lossy().into_owned())
        })
}

fn frontmatter_filter_metadata(raw: &serde_yaml_ng::Value) -> (Option<String>, Vec<String>) {
    let mut dcc = None;
    let mut tags = Vec::new();

    for (key, value) in collect_dcc_mcp_overrides(raw) {
        match key.as_str() {
            "dcc" if dcc.is_none() => {
                dcc = value.as_str().map(str::to_string);
            }
            "tags" if tags.is_empty() => {
                tags = parse_csv_or_list(&value);
            }
            _ => {}
        }
    }

    let Some(map) = raw.as_mapping() else {
        return (dcc, tags);
    };
    if dcc.is_none() {
        dcc = map
            .get(serde_yaml_ng::Value::String("dcc".to_string()))
            .and_then(|value| value.as_str())
            .map(str::to_string);
    }
    if tags.is_empty()
        && let Some(value) = map.get(serde_yaml_ng::Value::String("tags".to_string()))
    {
        tags = parse_csv_or_list(value);
    }

    (dcc, tags)
}

fn non_spec_top_level_suggestion(offending: &[String]) -> String {
    let mut replacements = Vec::new();
    for key in offending {
        let replacement = match key.as_str() {
            "dcc" => "metadata.dcc-mcp.dcc",
            "version" => "metadata.dcc-mcp.version",
            "tags" => "metadata.dcc-mcp.tags",
            "tools" => "metadata.dcc-mcp.tools: tools.yaml",
            "groups" => "metadata.dcc-mcp.groups",
            "prompts" => "metadata.dcc-mcp.prompts",
            "resources" => "metadata.dcc-mcp.resources",
            "depends" => "metadata.dcc-mcp.depends",
            "layer" => "metadata.dcc-mcp.layer",
            "stage" => "metadata.dcc-mcp.stage",
            "search-hint" | "search_hint" => "metadata.dcc-mcp.search-hint",
            "search-aliases" | "search_aliases" | "aliases" => "metadata.dcc-mcp.search-aliases",
            "products" => "metadata.dcc-mcp.products",
            "allow-implicit-invocation" | "allow_implicit_invocation" => {
                "metadata.dcc-mcp.allow-implicit-invocation"
            }
            "external-deps" | "external_deps" => "metadata.dcc-mcp.external-deps",
            "runtimes" | "runtime-deps" | "optional-runtimes" => "metadata.dcc-mcp.runtimes",
            "recipes" => "metadata.dcc-mcp.recipes",
            "introspection" => "metadata.dcc-mcp.introspection",
            "branding" => "metadata.dcc-mcp.branding",
            "links" => "metadata.dcc-mcp.links",
            "example-prompts" | "example_prompts" => "metadata.dcc-mcp.example-prompts",
            _ => "metadata.dcc-mcp.<key>",
        };
        replacements.push(format!("{key} -> {replacement}"));
    }

    if replacements.is_empty() {
        "Move dcc-mcp-core extensions under metadata.dcc-mcp.*.".to_string()
    } else {
        format!(
            "Move unsupported top-level key(s): {}. For version metadata, use metadata.dcc-mcp.version: \"1.0.0\".",
            replacements.join(", ")
        )
    }
}

// ── Issue #356: agentskills.io-compliant metadata.dcc-mcp.* support ──

/// Apply `metadata.dcc-mcp.*` overrides onto `meta`.
///
/// Missing keys leave the corresponding field at its serde default.
/// Sibling-file references for `tools` / `groups` are resolved relative
/// to `skill_dir`.
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
            "search-hint" => {
                if let Some(s) = value.as_str() {
                    meta.search_hint = s.to_string();
                }
            }
            "search-aliases" | "search_aliases" | "aliases" => {
                meta.search_aliases = parse_csv_or_list(&value);
            }
            "depends" => {
                meta.depends = parse_csv_or_list(&value);
            }
            "products" => {
                let products = parse_csv_or_list(&value);
                let policy = meta.policy.get_or_insert_with(Default::default);
                policy.products = products;
            }
            "allow-implicit-invocation" => {
                if let Some(b) = parse_bool_yaml(&value) {
                    let policy = meta.policy.get_or_insert_with(Default::default);
                    policy.allow_implicit_invocation = Some(b);
                }
            }
            "external-deps" => {
                if let Some(deps) = parse_external_deps_yaml(&value) {
                    meta.external_deps = Some(deps);
                }
            }
            "runtimes" | "runtime-deps" | "optional-runtimes" => {
                let runtimes = parse_runtime_descriptors_yaml(&value).or_else(|| {
                    value
                        .as_str()
                        .and_then(|rel| load_sibling_runtimes_file(skill_dir, rel))
                });
                if let Some(runtimes) = runtimes {
                    meta.runtimes = runtimes;
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
            "resources" => {
                // Sibling-file reference for the MCP resources primitive.
                // Parsing is deferred; the HTTP/MCP resource registry loads
                // sidecar YAML files lazily when resources are synchronized.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.resources_file = Some(s.to_string());
                }
            }
            "layer" => {
                // Architectural layer for skill routing and search partitioning.
                // Valid values: "infrastructure", "domain", "example".
                // See the skill layer taxonomy in AGENTS.md.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.layer = Some(s.to_string());
                }
            }
            "stage" => {
                // Pipeline stage tag for orchestration / minimal-mode presets.
                // Free-form string — each DCC adapter owns its own stage
                // vocabulary (e.g. Maya: bootstrap / scene / authoring /
                // interchange / pipeline). Core stays vocabulary-agnostic so
                // adapters don't have to upstream every new stage.
                if let Some(s) = value.as_str()
                    && !s.is_empty()
                {
                    meta.stage = Some(s.to_string());
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
            "branding" => {
                // Marketplace-card branding — colours, emoji, logo, tagline.
                // Drives the Admin UI Skills panel cards (Track D / #1407).
                if let Ok(branding) =
                    serde_yaml_ng::from_value::<dcc_mcp_models::SkillBranding>(value.clone())
                    && !branding.is_empty()
                {
                    meta.branding = Some(branding);
                }
            }
            "links" => {
                // External references — docs, repo, homepage, issues, chat.
                if let Ok(links) =
                    serde_yaml_ng::from_value::<dcc_mcp_models::SkillLinks>(value.clone())
                    && !links.is_empty()
                {
                    meta.links = Some(links);
                }
            }
            "example-prompts" | "example_prompts" => {
                meta.example_prompts = parse_csv_or_list(&value);
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
        // Canonical agentskills.io-compliant shape (issue #356) and the
        // shape produced by the sibling-file migration tool:
        //   `metadata: { dcc-mcp: { dcc: maya, ... } }`.
        //
        // The legacy flat form `metadata: { "dcc-mcp.dcc": "maya" }` used
        // in pre-0.15 skills is no longer accepted. Authors should use the
        // nested form above; see `docs/guide/skills.md` for the migration.
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

/// Parse optional runtime descriptors from `metadata.dcc-mcp.runtimes`.
fn parse_runtime_descriptors_yaml(
    v: &serde_yaml_ng::Value,
) -> Option<Vec<dcc_mcp_models::SkillRuntimeDescriptor>> {
    if let Some(s) = v.as_str() {
        return serde_json::from_str(s).ok();
    }
    serde_yaml_ng::from_value(v.clone()).ok()
}

/// Load optional runtime descriptors from a sibling YAML file referenced by
/// `metadata.dcc-mcp.runtimes`.
///
/// The file may either be a bare list or a mapping with a top-level
/// `runtimes:` key.
fn load_sibling_runtimes_file(
    skill_dir: &Path,
    rel: &str,
) -> Option<Vec<dcc_mcp_models::SkillRuntimeDescriptor>> {
    if !has_yaml_extension(rel) {
        tracing::warn!(
            "metadata.dcc-mcp.runtimes references {rel:?} which is not a .yaml/.yml file; ignoring"
        );
        return None;
    }
    let path = skill_dir.join(rel);
    let text = match std::fs::read_to_string(&path) {
        Ok(t) => t,
        Err(e) => {
            tracing::warn!(
                "failed to read sibling runtimes file {}: {e}",
                path.display()
            );
            return None;
        }
    };

    let value: serde_yaml_ng::Value = match serde_yaml_ng::from_str(&text) {
        Ok(value) => value,
        Err(e) => {
            tracing::warn!(
                "failed to parse sibling runtimes file {}: {e}",
                path.display()
            );
            return None;
        }
    };
    let runtimes_value = match value {
        serde_yaml_ng::Value::Mapping(map) => map
            .get(serde_yaml_ng::Value::String("runtimes".to_string()))
            .cloned()
            .unwrap_or(serde_yaml_ng::Value::Mapping(map)),
        other => other,
    };
    parse_runtime_descriptors_yaml(&runtimes_value)
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
