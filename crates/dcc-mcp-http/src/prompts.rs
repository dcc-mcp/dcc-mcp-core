//! MCP Prompts primitive (issues #351, #355).
//!
//! Exposes skill-authored prompt templates and workflow-derived prompts over
//! the MCP 2025-03-26 `prompts/list` + `prompts/get` JSON-RPC methods. The
//! capability is advertised only when [`crate::McpHttpConfig::enable_prompts`]
//! is `true`.
//!
//! # Sibling-file pattern (issue #356)
//!
//! Prompts live **outside** `SKILL.md`. A skill opts in by setting
//! `metadata.dcc-mcp.prompts: prompts.yaml` in its SKILL.md frontmatter;
//! `prompts.yaml` is a sibling file describing zero or more prompts and,
//! optionally, a list of workflows whose execution summary should be
//! rendered as an auto-generated prompt.
//!
//! ```yaml
//! # my-skill/prompts.yaml
//! prompts:
//!   - name: bake_and_export_animation
//!     description: Guide for baking a rig and exporting FBX
//!     arguments:
//!       - name: frame_range
//!         required: true
//!       - name: output_path
//!         required: true
//!     template: |
//!       Please bake the animation on the selected rig across frames
//!       {{frame_range}}. Then export to FBX at {{output_path}}.
//! workflows:
//!   - file: workflows/bake_and_export.yaml
//!     prompt_name: bake_and_export_workflow
//! ```
//!
//! # Template engine
//!
//! A minimal `{{name}}` substitutor (see [`render_template`]). Missing
//! arguments raise [`PromptError::MissingArg`] so `prompts/get` can
//! surface a descriptive JSON-RPC error naming the offending parameter.

use std::collections::{BTreeMap, HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};

use crate::protocol::{
    GetPromptResult, McpPrompt, McpPromptArgument, McpPromptContent, McpPromptMessage,
};

// ── Errors ──────────────────────────────────────────────────────────────────

/// Error type surfaced by [`PromptRegistry::get`].
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    #[error("prompt not found: {0}")]
    NotFound(String),
    #[error("missing required argument: {0}")]
    MissingArg(String),
    #[error("failed to load prompt source: {0}")]
    Load(String),
}

pub type PromptResult<T> = Result<T, PromptError>;

// ── Spec types (parsed from sibling YAML) ───────────────────────────────────

/// Declared argument for a hand-authored prompt.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptArgumentSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub required: bool,
}

/// Single prompt entry inside a sibling `prompts.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PromptSpec {
    pub name: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub arguments: Vec<PromptArgumentSpec>,
    pub template: String,
}

/// Reference to a workflow that should be surfaced as an auto-generated prompt.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct WorkflowPromptRef {
    /// Path to the workflow YAML (relative to the skill root).
    pub file: String,
    /// Public prompt name. When omitted, `{skill}.{workflow.name}` is used.
    #[serde(default)]
    pub prompt_name: Option<String>,
}

/// Parsed contents of a skill's `prompts.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default)]
pub struct PromptsSpec {
    #[serde(default)]
    pub prompts: Vec<PromptSpec>,
    #[serde(default)]
    pub workflows: Vec<WorkflowPromptRef>,
}

impl PromptsSpec {
    /// Parse a YAML document into a [`PromptsSpec`].
    pub fn from_yaml(s: &str) -> Result<Self, String> {
        serde_yaml_ng::from_str(s).map_err(|e| e.to_string())
    }
}

// ── Template engine ────────────────────────────────────────────────────────

/// Render a `{{name}}` template against an argument map.
///
/// Substitution rules:
///
/// - `{{name}}` → value from `args[name]`; missing → [`PromptError::MissingArg`].
/// - Whitespace inside the placeholder is tolerated: `{{ name }}`.
/// - Literal `{{` or `}}` outside a placeholder are preserved verbatim.
/// - The engine does NOT support `{{{raw}}}`, filters, blocks, or
///   conditionals (Handlebars-style). Keep templates simple.
///
/// # Errors
///
/// Returns the first missing-argument name encountered.
pub fn render_template(template: &str, args: &HashMap<String, String>) -> PromptResult<String> {
    let bytes = template.as_bytes();
    let mut out = String::with_capacity(template.len());
    let mut i = 0;
    while i < bytes.len() {
        // Look for `{{`
        if i + 1 < bytes.len() && bytes[i] == b'{' && bytes[i + 1] == b'{' {
            if let Some(end_rel) = template[i + 2..].find("}}") {
                let raw = &template[i + 2..i + 2 + end_rel];
                let name = raw.trim();
                if !name.is_empty() && is_valid_placeholder(name) {
                    match args.get(name) {
                        Some(v) => out.push_str(v),
                        None => return Err(PromptError::MissingArg(name.to_string())),
                    }
                    i = i + 2 + end_rel + 2;
                    continue;
                }
                // Not a valid placeholder — emit `{{` literally and advance by 1.
                out.push_str("{{");
                i += 2;
                continue;
            }
        }
        // Regular char — UTF-8 safe advance.
        let ch_end = next_char_boundary(template, i);
        out.push_str(&template[i..ch_end]);
        i = ch_end;
    }
    Ok(out)
}

fn is_valid_placeholder(s: &str) -> bool {
    s.chars()
        .all(|c| c.is_ascii_alphanumeric() || c == '_' || c == '-' || c == '.')
}

fn next_char_boundary(s: &str, start: usize) -> usize {
    // Walk forward to the next UTF-8 char boundary.
    let mut j = start + 1;
    while j < s.len() && !s.is_char_boundary(j) {
        j += 1;
    }
    j
}

// ── Registry ────────────────────────────────────────────────────────────────

/// How a prompt was sourced — surfaced to downstream consumers for
/// diagnostics / client hints. Never emitted on the wire.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSource {
    /// From a `*.prompt.yaml` / `prompts.yaml` sibling file.
    Explicit,
    /// Derived from a workflow referenced in the sibling `workflows:` section.
    WorkflowDerived,
}

/// A single entry in the prompt registry — pre-rendered list metadata plus
/// the raw template used by `prompts/get`.
#[derive(Debug, Clone)]
pub struct PromptEntry {
    pub name: String,
    pub description: Option<String>,
    pub arguments: Vec<PromptArgumentSpec>,
    pub template: String,
    pub source: PromptSource,
    /// Fully-qualified skill name this prompt belongs to.
    pub skill: String,
}

impl PromptEntry {
    fn to_mcp(&self) -> McpPrompt {
        McpPrompt {
            name: self.name.clone(),
            description: self.description.clone(),
            arguments: self
                .arguments
                .iter()
                .map(|a| McpPromptArgument {
                    name: a.name.clone(),
                    description: a.description.clone(),
                    required: a.required,
                })
                .collect(),
        }
    }
}

/// Thread-safe registry of prompts, rebuilt lazily on demand.
///
/// Owned by [`crate::handler::AppState`] via `Arc`. The registry caches the
/// set of loaded skills it last saw; when `prompts/list` or `prompts/get` is
/// called it rescans the [`dcc_mcp_skills::SkillCatalog`] (the heavy work —
/// actually opening and parsing sibling YAML files — happens once per skill
/// per cache invalidation).
#[derive(Clone, Default)]
pub struct PromptRegistry {
    inner: Arc<RwLock<PromptRegistryInner>>,
}

#[derive(Default)]
struct PromptRegistryInner {
    /// Prompts keyed by (skill_name, prompt_name) so duplicate names across
    /// skills don't collide.
    entries: BTreeMap<(String, String), PromptEntry>,
    /// Names of the loaded skills this cache was built from. Used to
    /// short-circuit rebuilds — swapping this out atomically invalidates.
    loaded_skills: HashSet<String>,
    enabled: bool,
}

impl PromptRegistry {
    /// Create an empty registry.
    ///
    /// Pass `enabled = false` to disable the whole primitive — `list()` will
    /// always return an empty `Vec` and `get()` will return
    /// [`PromptError::NotFound`] for every name.
    pub fn new(enabled: bool) -> Self {
        Self {
            inner: Arc::new(RwLock::new(PromptRegistryInner {
                enabled,
                ..Default::default()
            })),
        }
    }

    /// Returns `true` when the Prompts primitive is advertised in
    /// `initialize` (mirrors `McpHttpConfig::enable_prompts`).
    pub fn is_enabled(&self) -> bool {
        self.inner.read().enabled
    }

    /// Force a rebuild on next `list` / `get`.
    ///
    /// Called by the server when a skill is loaded or unloaded — the cached
    /// entry set is cleared so the next request rescans all loaded skills.
    pub fn invalidate(&self) {
        let mut g = self.inner.write();
        g.entries.clear();
        g.loaded_skills.clear();
    }

    /// List every prompt known to the registry.
    pub fn list<F>(&self, walk_loaded: F) -> Vec<McpPrompt>
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return Vec::new();
        }
        self.refresh_if_needed(walk_loaded);
        self.inner
            .read()
            .entries
            .values()
            .map(PromptEntry::to_mcp)
            .collect()
    }

    /// Look up + render a single prompt.
    pub fn get<F>(
        &self,
        name: &str,
        args: &HashMap<String, String>,
        walk_loaded: F,
    ) -> PromptResult<GetPromptResult>
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        if !self.is_enabled() {
            return Err(PromptError::NotFound(name.to_string()));
        }
        self.refresh_if_needed(walk_loaded);
        // Validate required args present before rendering so we fail fast
        // with a named-parameter error.
        let entry = {
            let g = self.inner.read();
            g.entries
                .values()
                .find(|e| e.name == name)
                .cloned()
                .ok_or_else(|| PromptError::NotFound(name.to_string()))?
        };
        for arg in &entry.arguments {
            if arg.required && !args.contains_key(&arg.name) {
                return Err(PromptError::MissingArg(arg.name.clone()));
            }
        }
        let rendered = render_template(&entry.template, args)?;
        Ok(GetPromptResult {
            description: entry.description.clone(),
            messages: vec![McpPromptMessage {
                role: "user".to_string(),
                content: McpPromptContent::text(rendered),
            }],
        })
    }

    fn refresh_if_needed<F>(&self, walk_loaded: F)
    where
        F: FnOnce(&mut dyn FnMut(&dcc_mcp_models::SkillMetadata)),
    {
        // Collect loaded skills.
        let mut loaded_now: HashSet<String> = HashSet::new();
        let mut metadatas: Vec<dcc_mcp_models::SkillMetadata> = Vec::new();
        let mut cb = |md: &dcc_mcp_models::SkillMetadata| {
            loaded_now.insert(md.name.clone());
            metadatas.push(md.clone());
        };
        walk_loaded(&mut cb);

        // Fast path — loaded skill set unchanged and cache populated.
        {
            let g = self.inner.read();
            if g.loaded_skills == loaded_now && !g.entries.is_empty() {
                return;
            }
            // Empty cache + empty loaded set → still fine, mark set.
            if g.loaded_skills == loaded_now && loaded_now.is_empty() {
                return;
            }
        }

        // Slow path — rebuild.
        let mut new_entries: BTreeMap<(String, String), PromptEntry> = BTreeMap::new();
        for md in &metadatas {
            if let Some(rel) = md.prompts_file.as_deref() {
                let skill_path = PathBuf::from(&md.skill_path);
                for entry in load_prompts_from_reference(&skill_path, rel, &md.name) {
                    new_entries.insert((md.name.clone(), entry.name.clone()), entry);
                }
            }
        }
        let mut g = self.inner.write();
        g.entries = new_entries;
        g.loaded_skills = loaded_now;
    }
}

/// Resolve `reference` (relative to `skill_root`) and return every prompt
/// entry it produces. `reference` may point at a single YAML file or at a
/// glob (`prompts/*.prompt.yaml`).
fn load_prompts_from_reference(
    skill_root: &Path,
    reference: &str,
    skill_name: &str,
) -> Vec<PromptEntry> {
    let mut out = Vec::new();

    if reference.contains('*') || reference.contains('?') {
        // Glob — treat each matching file as a standalone PromptSpec.
        let pattern_root = match reference.split_once('/') {
            Some((dir, _)) if !dir.contains('*') && !dir.contains('?') => skill_root.join(dir),
            _ => skill_root.to_path_buf(),
        };
        if let Ok(rd) = std::fs::read_dir(&pattern_root) {
            for ent in rd.flatten() {
                let p = ent.path();
                if p.extension()
                    .and_then(|e| e.to_str())
                    .map(|e| matches!(e, "yaml" | "yml"))
                    .unwrap_or(false)
                {
                    let Ok(text) = std::fs::read_to_string(&p) else {
                        continue;
                    };
                    if let Ok(spec) = serde_yaml_ng::from_str::<PromptSpec>(&text) {
                        out.push(entry_from_spec(spec, skill_name));
                    } else if let Ok(spec) = serde_yaml_ng::from_str::<PromptsSpec>(&text) {
                        out.extend(entries_from_promptsspec(spec, skill_root, skill_name));
                    }
                }
            }
        }
        return out;
    }

    // Single file — expected to contain a PromptsSpec with prompts+workflows.
    let path = skill_root.join(reference);
    let Ok(text) = std::fs::read_to_string(&path) else {
        tracing::warn!(
            "prompts sibling file {} missing or unreadable; skipping",
            path.display()
        );
        return out;
    };
    match PromptsSpec::from_yaml(&text) {
        Ok(spec) => {
            out.extend(entries_from_promptsspec(spec, skill_root, skill_name));
        }
        Err(e) => {
            tracing::warn!("failed to parse {}: {e}", path.display());
        }
    }
    out
}

fn entry_from_spec(spec: PromptSpec, skill_name: &str) -> PromptEntry {
    PromptEntry {
        name: spec.name,
        description: spec.description,
        arguments: spec.arguments,
        template: spec.template,
        source: PromptSource::Explicit,
        skill: skill_name.to_string(),
    }
}

fn entries_from_promptsspec(
    spec: PromptsSpec,
    skill_root: &Path,
    skill_name: &str,
) -> Vec<PromptEntry> {
    let mut out: Vec<PromptEntry> = spec
        .prompts
        .into_iter()
        .map(|p| entry_from_spec(p, skill_name))
        .collect();

    for wf_ref in spec.workflows {
        if let Some(entry) = workflow_to_prompt(skill_root, skill_name, &wf_ref) {
            out.push(entry);
        }
    }
    out
}

/// Load a workflow YAML and render a prompt entry that summarises its
/// purpose + step chain.
fn workflow_to_prompt(
    skill_root: &Path,
    skill_name: &str,
    wf_ref: &WorkflowPromptRef,
) -> Option<PromptEntry> {
    let path = skill_root.join(&wf_ref.file);
    let text = std::fs::read_to_string(&path)
        .map_err(|e| {
            tracing::warn!(
                "workflow-derived prompt: failed to read {}: {e}",
                path.display()
            )
        })
        .ok()?;
    let spec = dcc_mcp_workflow::WorkflowSpec::from_yaml(&text)
        .map_err(|e| {
            tracing::warn!(
                "workflow-derived prompt: failed to parse {}: {e}",
                path.display()
            )
        })
        .ok()?;

    let name = wf_ref
        .prompt_name
        .clone()
        .unwrap_or_else(|| format!("{skill_name}.{}", spec.name));

    let mut steps_text = String::new();
    for (i, step) in spec.steps.iter().enumerate() {
        let (tool, kind) = match &step.kind {
            dcc_mcp_workflow::StepKind::Tool { tool, .. } => (tool.as_str(), "tool"),
            dcc_mcp_workflow::StepKind::ToolRemote { tool, .. } => (tool.as_str(), "tool_remote"),
            other => ("", other.kind_str()),
        };
        if tool.is_empty() {
            steps_text.push_str(&format!("  {}. [{}] {}\n", i + 1, kind, step.id));
        } else {
            steps_text.push_str(&format!("  {}. {}\n", i + 1, tool));
        }
    }

    let template = format!(
        "Workflow: {description}\n\nThis workflow runs these steps in order:\n{steps}\n\
         Call `workflows.run` with name = {wf_name:?} to execute, or invoke the tools \
         manually in the order above.\n",
        description = if spec.description.is_empty() {
            spec.name.clone()
        } else {
            spec.description.clone()
        },
        steps = steps_text,
        wf_name = spec.name,
    );

    Some(PromptEntry {
        name,
        description: Some(format!(
            "Auto-generated prompt describing the {} workflow.",
            spec.name
        )),
        arguments: Vec::new(),
        template,
        source: PromptSource::WorkflowDerived,
        skill: skill_name.to_string(),
    })
}

// ── Tests ───────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn argmap(pairs: &[(&str, &str)]) -> HashMap<String, String> {
        pairs
            .iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect()
    }

    #[test]
    fn render_basic() {
        let t = "hello {{name}}";
        let out = render_template(t, &argmap(&[("name", "world")])).unwrap();
        assert_eq!(out, "hello world");
    }

    #[test]
    fn render_no_args() {
        let out = render_template("plain string", &HashMap::new()).unwrap();
        assert_eq!(out, "plain string");
    }

    #[test]
    fn render_multi_arg() {
        let t = "{{a}}+{{b}}={{c}}";
        let out = render_template(t, &argmap(&[("a", "1"), ("b", "2"), ("c", "3")])).unwrap();
        assert_eq!(out, "1+2=3");
    }

    #[test]
    fn render_missing_arg_errors() {
        let t = "hi {{who}}";
        let err = render_template(t, &HashMap::new()).unwrap_err();
        assert!(matches!(err, PromptError::MissingArg(ref s) if s == "who"));
    }

    #[test]
    fn render_duplicate_arg() {
        let t = "{{x}}-{{x}}-{{x}}";
        let out = render_template(t, &argmap(&[("x", "42")])).unwrap();
        assert_eq!(out, "42-42-42");
    }

    #[test]
    fn render_tolerates_whitespace_inside_braces() {
        let t = "v={{  k  }}";
        let out = render_template(t, &argmap(&[("k", "ok")])).unwrap();
        assert_eq!(out, "v=ok");
    }

    #[test]
    fn render_preserves_unmatched_open_braces() {
        // Dangling `{{` with no closing `}}` is kept verbatim.
        let t = "weird {{ no close";
        let out = render_template(t, &HashMap::new()).unwrap();
        assert_eq!(out, "weird {{ no close");
    }

    #[test]
    fn render_preserves_non_placeholder_brace_content() {
        // `{{ }}` containing invalid placeholder chars is left intact.
        let t = "code: {{ 1 + 1 }} done";
        let out = render_template(t, &HashMap::new()).unwrap();
        assert_eq!(out, "code: {{ 1 + 1 }} done");
    }

    #[test]
    fn render_empty_template() {
        let out = render_template("", &HashMap::new()).unwrap();
        assert_eq!(out, "");
    }

    #[test]
    fn registry_disabled_returns_empty() {
        let reg = PromptRegistry::new(false);
        let list = reg.list(|_| {});
        assert!(list.is_empty());
        let err = reg.get("foo", &HashMap::new(), |_| {}).unwrap_err();
        assert!(matches!(err, PromptError::NotFound(_)));
    }

    #[test]
    fn promptsspec_from_yaml_parses_both_sections() {
        let yaml = r#"
prompts:
  - name: one
    description: first
    arguments:
      - name: x
        required: true
    template: "x is {{x}}"
workflows:
  - file: wf/bake.yaml
    prompt_name: bake_summary
"#;
        let spec = PromptsSpec::from_yaml(yaml).unwrap();
        assert_eq!(spec.prompts.len(), 1);
        assert_eq!(spec.workflows.len(), 1);
        assert_eq!(spec.prompts[0].name, "one");
        assert_eq!(
            spec.workflows[0].prompt_name.as_deref(),
            Some("bake_summary")
        );
    }
}
