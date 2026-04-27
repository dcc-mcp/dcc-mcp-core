//! Sibling-file loader + `PromptEntry` model for [`crate::prompts`].

use std::path::Path;

use super::spec::{PromptArgumentSpec, PromptSpec, PromptsSpec, WorkflowPromptRef};
use crate::protocol::{McpPrompt, McpPromptArgument};

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
        }
    }
}

/// Resolve `reference` (relative to `skill_root`) and return every prompt
/// entry it produces. `reference` may point at a single YAML file or at a
/// glob (`prompts/*.prompt.yaml`).
pub(crate) fn load_prompts_from_reference(
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
