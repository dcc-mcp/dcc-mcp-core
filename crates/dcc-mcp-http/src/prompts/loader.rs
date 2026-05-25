//! Sibling-file loader + `PromptEntry` model for [`crate::prompts`].

use std::path::{Path, PathBuf};

use super::spec::{PromptArgumentSpec, PromptSpec, PromptsSpec, WorkflowPromptRef};
use dcc_mcp_jsonrpc::{McpPrompt, McpPromptArgument};
use dcc_mcp_models::SkillMetadata;
use serde::Serialize;
use serde_json::{Value, json};

const METADATA_KEY_EXAMPLES: &str = "dcc-mcp.examples";
const METADATA_KEY_WORKFLOWS: &str = "dcc-mcp.workflows";
const PROMPT_SOURCE_META_KEY: &str = "dcc.prompt_source";
const MAX_DERIVED_PROMPT_CHARS: usize = 16_384;

/// How a prompt was sourced — surfaced to downstream consumers for
/// diagnostics / client hints.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PromptSource {
    /// From a `*.prompt.yaml` / `prompts.yaml` sibling file.
    Explicit,
    /// Derived from a workflow referenced in the sibling `workflows:` section.
    WorkflowDerived,
    /// Derived from `metadata.dcc-mcp.workflows`.
    WorkflowMetadataDerived,
    /// Derived from `metadata.dcc-mcp.examples`.
    ExamplesDerived,
    /// Derived from `metadata.dcc-mcp.recipes`.
    RecipesDerived,
}

impl PromptSource {
    /// Stable string surfaced in prompt `_meta` and diagnostics.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Explicit => "explicit",
            Self::WorkflowDerived => "workflow",
            Self::WorkflowMetadataDerived => "workflow_metadata",
            Self::ExamplesDerived => "examples",
            Self::RecipesDerived => "recipes",
        }
    }
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
    pub(crate) fn to_mcp(&self) -> McpPrompt {
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
            meta: Some(json!({
                PROMPT_SOURCE_META_KEY: {
                    "skill": self.skill,
                    "source": self.source.as_str(),
                }
            })),
        }
    }
}

/// Diagnostic snapshot returned with empty `prompts/list` responses.
#[derive(Debug, Clone, Default, Serialize, PartialEq, Eq)]
pub struct PromptDiagnostics {
    /// Whether the prompt primitive is enabled in server configuration.
    pub enabled: bool,
    /// Number of currently loaded skills inspected by the registry.
    pub loaded_skill_count: usize,
    /// Total prompts returned by the registry, including manual registrations.
    pub prompt_count: usize,
    /// Prompt count sourced from loaded skills.
    pub skill_prompt_count: usize,
    /// Prompt count registered programmatically by an embedder.
    pub manual_prompt_count: usize,
    /// Loaded skills that explicitly declared `metadata.dcc-mcp.prompts`.
    pub explicit_skill_count: usize,
    /// Loaded skills that produced at least one derived prompt.
    pub derived_skill_count: usize,
    /// Loaded skills that had prompt-worthy metadata, even if loading failed.
    pub prompt_capable_skill_count: usize,
    /// Prompt source entries, one per prompt.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub sources: Vec<PromptDiagnosticSource>,
    /// Missing files, parse failures, or invalid metadata references.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub failures: Vec<PromptDiagnosticFailure>,
    /// Human-readable hints for empty-list troubleshooting.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub notes: Vec<String>,
}

/// Source record for a prompt entry.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptDiagnosticSource {
    pub skill: String,
    pub prompt: String,
    pub source: String,
}

/// One prompt-loading diagnostic failure.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
pub struct PromptDiagnosticFailure {
    pub skill: String,
    pub reference: String,
    pub kind: String,
    pub message: String,
}

impl PromptDiagnosticFailure {
    fn new(
        skill: &str,
        reference: impl Into<String>,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) -> Self {
        Self {
            skill: skill.to_string(),
            reference: reference.into(),
            kind: kind.into(),
            message: message.into(),
        }
    }
}

/// Internal loader result.
#[derive(Debug, Default)]
pub(crate) struct PromptLoadReport {
    pub entries: Vec<PromptEntry>,
    pub failures: Vec<PromptDiagnosticFailure>,
    pub prompt_capable: bool,
}

/// Resolve `reference` (relative to `skill_root`) and return every prompt
/// entry it produces. `reference` may point at a single YAML file or at a
/// glob (`prompts/*.prompt.yaml`).
pub(crate) fn load_prompts_from_reference(
    skill_root: &Path,
    reference: &str,
    skill_name: &str,
) -> PromptLoadReport {
    let mut report = PromptLoadReport {
        prompt_capable: true,
        ..Default::default()
    };

    if reference.contains('*') || reference.contains('?') {
        // Glob — treat each matching file as a standalone PromptSpec.
        let pattern_root = wildcard_base_dir(skill_root, reference);
        let rd = match std::fs::read_dir(&pattern_root) {
            Ok(rd) => rd,
            Err(e) => {
                report.failures.push(PromptDiagnosticFailure::new(
                    skill_name,
                    reference,
                    "read",
                    format!(
                        "failed to read prompt directory {}: {e}",
                        pattern_root.display()
                    ),
                ));
                return report;
            }
        };
        for ent in rd.flatten() {
            let p = ent.path();
            if !has_ext(&p, &["yaml", "yml"]) {
                continue;
            }
            let Ok(text) = std::fs::read_to_string(&p) else {
                report.failures.push(PromptDiagnosticFailure::new(
                    skill_name,
                    p.display().to_string(),
                    "read",
                    "failed to read prompt file",
                ));
                continue;
            };
            if let Ok(spec) = serde_yaml_ng::from_str::<PromptSpec>(&text) {
                report.entries.push(entry_from_spec(spec, skill_name));
            } else if let Ok(spec) = serde_yaml_ng::from_str::<PromptsSpec>(&text) {
                let child = entries_from_promptsspec(spec, skill_root, skill_name);
                report.entries.extend(child.entries);
                report.failures.extend(child.failures);
            } else {
                report.failures.push(PromptDiagnosticFailure::new(
                    skill_name,
                    p.display().to_string(),
                    "parse",
                    "failed to parse prompt file as PromptSpec or PromptsSpec",
                ));
            }
        }
        return report;
    }

    // Single file — expected to contain a PromptsSpec with prompts+workflows.
    let path = skill_root.join(reference);
    let Ok(text) = std::fs::read_to_string(&path) else {
        tracing::warn!(
            "prompts sibling file {} missing or unreadable; skipping",
            path.display()
        );
        report.failures.push(PromptDiagnosticFailure::new(
            skill_name,
            reference,
            "read",
            format!(
                "prompts sibling file {} missing or unreadable",
                path.display()
            ),
        ));
        return report;
    };
    match PromptsSpec::from_yaml(&text) {
        Ok(spec) => {
            let child = entries_from_promptsspec(spec, skill_root, skill_name);
            report.entries.extend(child.entries);
            report.failures.extend(child.failures);
        }
        Err(e) => {
            tracing::warn!("failed to parse {}: {e}", path.display());
            report.failures.push(PromptDiagnosticFailure::new(
                skill_name,
                reference,
                "parse",
                format!("failed to parse prompts sibling file: {e}"),
            ));
        }
    }
    report
}

/// Derive useful prompts from prompt-worthy skill metadata when the skill did
/// not declare an explicit `metadata.dcc-mcp.prompts` sibling file.
pub(crate) fn load_derived_prompts_from_skill(meta: &SkillMetadata) -> PromptLoadReport {
    let skill_root = PathBuf::from(&meta.skill_path);
    let mut report = PromptLoadReport::default();

    let examples_declared = meta.metadata.get(METADATA_KEY_EXAMPLES).is_some();
    let example_refs = metadata_string_refs(&meta.metadata, METADATA_KEY_EXAMPLES);
    if examples_declared && example_refs.is_empty() {
        report.prompt_capable = true;
        report.failures.push(PromptDiagnosticFailure::new(
            &meta.name,
            METADATA_KEY_EXAMPLES,
            "metadata",
            "metadata.dcc-mcp.examples must be a non-empty string or string list",
        ));
    } else if !example_refs.is_empty() {
        report.prompt_capable = true;
        for reference in example_refs {
            let paths = match resolve_reference_paths(
                &skill_root,
                &reference,
                &["md", "txt"],
                &meta.name,
                "examples",
            ) {
                Ok(paths) => paths,
                Err(failure) => {
                    report.failures.push(failure);
                    continue;
                }
            };
            let multi = paths.len() > 1;
            for path in paths {
                match prompt_from_text_file(
                    &meta.name,
                    &path,
                    PromptSource::ExamplesDerived,
                    "examples",
                    multi,
                ) {
                    Ok(entry) => report.entries.push(entry),
                    Err(failure) => report.failures.push(failure),
                }
            }
        }
    }

    if let Some(reference) = meta.recipes_file.as_deref().filter(|s| !s.is_empty()) {
        report.prompt_capable = true;
        let paths = match resolve_reference_paths(
            &skill_root,
            reference,
            &["md", "txt", "yaml", "yml", "toml"],
            &meta.name,
            "recipes",
        ) {
            Ok(paths) => paths,
            Err(failure) => {
                report.failures.push(failure);
                Vec::new()
            }
        };
        let multi = paths.len() > 1;
        for path in paths {
            match prompt_from_text_file(
                &meta.name,
                &path,
                PromptSource::RecipesDerived,
                "recipes",
                multi,
            ) {
                Ok(entry) => report.entries.push(entry),
                Err(failure) => report.failures.push(failure),
            }
        }
    }

    if meta.metadata.get(METADATA_KEY_WORKFLOWS).is_some() {
        report.prompt_capable = true;
        match dcc_mcp_workflow::catalog::resolve_workflow_paths(meta, &skill_root) {
            Ok(paths) => {
                if paths.is_empty() {
                    report.failures.push(PromptDiagnosticFailure::new(
                        &meta.name,
                        METADATA_KEY_WORKFLOWS,
                        "missing",
                        "workflow metadata matched no workflow files",
                    ));
                }
                for path in paths {
                    match workflow_path_to_prompt(
                        &path,
                        &meta.name,
                        None,
                        PromptSource::WorkflowMetadataDerived,
                    ) {
                        Ok(entry) => report.entries.push(entry),
                        Err(failure) => report.failures.push(failure),
                    }
                }
            }
            Err(e) => report.failures.push(PromptDiagnosticFailure::new(
                &meta.name,
                METADATA_KEY_WORKFLOWS,
                "glob",
                format!("failed to resolve workflow metadata: {e}"),
            )),
        }
    }

    report
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
) -> PromptLoadReport {
    let mut report = PromptLoadReport {
        entries: spec
            .prompts
            .into_iter()
            .map(|p| entry_from_spec(p, skill_name))
            .collect(),
        prompt_capable: true,
        ..Default::default()
    };

    for wf_ref in spec.workflows {
        match workflow_to_prompt(skill_root, skill_name, &wf_ref) {
            Ok(entry) => report.entries.push(entry),
            Err(failure) => report.failures.push(failure),
        }
    }
    report
}

/// Load a workflow YAML and render a prompt entry that summarises its
/// purpose + step chain.
fn workflow_to_prompt(
    skill_root: &Path,
    skill_name: &str,
    wf_ref: &WorkflowPromptRef,
) -> Result<PromptEntry, PromptDiagnosticFailure> {
    workflow_path_to_prompt(
        &skill_root.join(&wf_ref.file),
        skill_name,
        wf_ref.prompt_name.clone(),
        PromptSource::WorkflowDerived,
    )
}

fn workflow_path_to_prompt(
    path: &Path,
    skill_name: &str,
    prompt_name: Option<String>,
    source: PromptSource,
) -> Result<PromptEntry, PromptDiagnosticFailure> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        tracing::warn!(
            "workflow-derived prompt: failed to read {}: {e}",
            path.display()
        );
        PromptDiagnosticFailure::new(
            skill_name,
            path.display().to_string(),
            "read",
            format!("failed to read workflow file: {e}"),
        )
    })?;
    let spec = dcc_mcp_workflow::WorkflowSpec::from_yaml(&text).map_err(|e| {
        tracing::warn!(
            "workflow-derived prompt: failed to parse {}: {e}",
            path.display()
        );
        PromptDiagnosticFailure::new(
            skill_name,
            path.display().to_string(),
            "parse",
            format!("failed to parse workflow file: {e}"),
        )
    })?;

    let name = prompt_name.unwrap_or_else(|| format!("{skill_name}.{}", spec.name));

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
         Call `workflows_run` with name = {wf_name:?} to execute, or invoke the tools \
         manually in the order above.\n",
        description = if spec.description.is_empty() {
            spec.name.clone()
        } else {
            spec.description.clone()
        },
        steps = steps_text,
        wf_name = spec.name,
    );

    Ok(PromptEntry {
        name,
        description: Some(format!(
            "Auto-generated prompt describing the {} workflow.",
            spec.name
        )),
        arguments: Vec::new(),
        template,
        source,
        skill: skill_name.to_string(),
    })
}

fn prompt_from_text_file(
    skill_name: &str,
    path: &Path,
    source: PromptSource,
    prefix: &str,
    include_stem: bool,
) -> Result<PromptEntry, PromptDiagnosticFailure> {
    let text = std::fs::read_to_string(path).map_err(|e| {
        PromptDiagnosticFailure::new(
            skill_name,
            path.display().to_string(),
            "read",
            format!("failed to read {prefix} file: {e}"),
        )
    })?;
    let content = trim_derived_prompt_content(&text);
    if content.trim().is_empty() {
        return Err(PromptDiagnosticFailure::new(
            skill_name,
            path.display().to_string(),
            "empty",
            format!("{prefix} file is empty"),
        ));
    }
    let stem = prompt_stem(path);
    let name = if include_stem {
        format!("{skill_name}.{prefix}.{stem}")
    } else {
        format!("{skill_name}.{prefix}")
    };
    let template = format!(
        "Use the `{skill_name}` skill with this {prefix} guidance. Prefer the \
         skill's declared MCP tools and keep DCC-specific side effects explicit.\n\n{content}"
    );
    Ok(PromptEntry {
        name,
        description: Some(format!(
            "Auto-generated prompt from {prefix} guidance for {skill_name}."
        )),
        arguments: Vec::new(),
        template,
        source,
        skill: skill_name.to_string(),
    })
}

fn metadata_string_refs(metadata: &Value, key: &str) -> Vec<String> {
    match metadata.get(key) {
        Some(Value::String(raw)) => split_refs(raw),
        Some(Value::Array(items)) => items
            .iter()
            .filter_map(Value::as_str)
            .flat_map(split_refs)
            .collect(),
        _ => Vec::new(),
    }
}

fn split_refs(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(ToOwned::to_owned)
        .collect()
}

fn resolve_reference_paths(
    skill_root: &Path,
    reference: &str,
    allowed_exts: &[&str],
    skill: &str,
    kind: &str,
) -> Result<Vec<PathBuf>, PromptDiagnosticFailure> {
    let path = skill_root.join(reference);
    if !reference.contains('*') && !reference.contains('?') {
        return if path.is_file() {
            Ok(vec![path])
        } else {
            Err(PromptDiagnosticFailure::new(
                skill,
                reference,
                "missing",
                format!("{kind} file {} does not exist", path.display()),
            ))
        };
    }

    let pattern_root = wildcard_base_dir(skill_root, reference);
    let rd = std::fs::read_dir(&pattern_root).map_err(|e| {
        PromptDiagnosticFailure::new(
            skill,
            reference,
            "read",
            format!(
                "failed to read {kind} directory {}: {e}",
                pattern_root.display()
            ),
        )
    })?;
    let mut paths = Vec::new();
    for ent in rd.flatten() {
        let p = ent.path();
        if p.is_file() && has_ext(&p, allowed_exts) {
            paths.push(p);
        }
    }
    paths.sort();
    if paths.is_empty() {
        return Err(PromptDiagnosticFailure::new(
            skill,
            reference,
            "missing",
            format!("{kind} reference matched no files"),
        ));
    }
    Ok(paths)
}

fn wildcard_base_dir(skill_root: &Path, reference: &str) -> PathBuf {
    let wildcard = reference.find(['*', '?']).unwrap_or(reference.len());
    let prefix = &reference[..wildcard];
    let dir = prefix
        .rfind(['/', '\\'])
        .map(|idx| &prefix[..idx])
        .unwrap_or("");
    if dir.is_empty() {
        skill_root.to_path_buf()
    } else {
        skill_root.join(dir)
    }
}

fn has_ext(path: &Path, allowed: &[&str]) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|ext| {
            allowed
                .iter()
                .any(|allowed| ext.eq_ignore_ascii_case(allowed))
        })
        .unwrap_or(false)
}

fn prompt_stem(path: &Path) -> String {
    let stem = path
        .file_stem()
        .and_then(|s| s.to_str())
        .unwrap_or("guidance")
        .trim_end_matches(".workflow");
    let mut out = String::new();
    for ch in stem.chars() {
        if ch.is_ascii_alphanumeric() || ch == '_' || ch == '-' {
            out.push(ch);
        } else {
            out.push('_');
        }
    }
    if out.is_empty() {
        "guidance".to_string()
    } else {
        out
    }
}

fn trim_derived_prompt_content(text: &str) -> String {
    let trimmed = text.trim();
    if trimmed.chars().count() <= MAX_DERIVED_PROMPT_CHARS {
        return trimmed.to_string();
    }
    let mut out: String = trimmed.chars().take(MAX_DERIVED_PROMPT_CHARS).collect();
    out.push_str("\n\n[content truncated]");
    out
}
