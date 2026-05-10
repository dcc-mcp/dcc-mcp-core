//! Pure wire/spec types for the MCP Prompts primitive.
//!
//! Runtime prompt registry, filesystem loading, and template rendering stay in
//! `dcc-mcp-http`.  This module only hosts serialisable prompt specification
//! types parsed from sibling `prompts.yaml` files plus the prompt error shape.

use serde::{Deserialize, Serialize};

/// Error type surfaced by prompt lookup and rendering.
#[must_use]
#[derive(Debug, thiserror::Error)]
pub enum PromptError {
    /// Prompt with the requested name was not found.
    #[error("prompt not found: {0}")]
    NotFound(String),
    /// Required argument was missing from the render request.
    #[error("missing required argument: {0}")]
    MissingArg(String),
    /// Prompt source failed to load or parse.
    #[error("failed to load prompt source: {0}")]
    Load(String),
}

/// Result alias for prompt operations.
pub type PromptResult<T> = Result<T, PromptError>;

/// Declared argument for a hand-authored prompt.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PromptArgumentSpec {
    /// Argument name used in `{{placeholder}}` template substitutions.
    pub name: String,
    /// Human-readable argument description surfaced by `prompts/list`.
    #[serde(default)]
    pub description: Option<String>,
    /// Whether this argument must be provided to `prompts/get`.
    #[serde(default)]
    pub required: bool,
}

/// Single prompt entry inside a sibling `prompts.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct PromptSpec {
    /// Public prompt name.
    pub name: String,
    /// Human-readable prompt description.
    #[serde(default)]
    pub description: Option<String>,
    /// Prompt arguments declared for this template.
    #[serde(default)]
    pub arguments: Vec<PromptArgumentSpec>,
    /// Raw prompt template text.
    pub template: String,
}

/// Reference to a workflow that should be surfaced as an auto-generated prompt.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct WorkflowPromptRef {
    /// Path to the workflow YAML (relative to the skill root).
    pub file: String,
    /// Public prompt name. When omitted, `{skill}.{workflow.name}` is used.
    #[serde(default)]
    pub prompt_name: Option<String>,
}

/// Parsed contents of a skill's `prompts.yaml`.
#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct PromptsSpec {
    /// Hand-authored prompt templates.
    #[serde(default)]
    pub prompts: Vec<PromptSpec>,
    /// Workflow YAML files surfaced as generated prompts.
    #[serde(default)]
    pub workflows: Vec<WorkflowPromptRef>,
}

impl PromptsSpec {
    /// Parse a YAML document into a [`PromptsSpec`].
    pub fn from_yaml(s: &str) -> Result<Self, String> {
        serde_yaml_ng::from_str(s).map_err(|e| e.to_string())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn prompt_error_display_is_stable() {
        assert_eq!(
            PromptError::NotFound("foo".to_owned()).to_string(),
            "prompt not found: foo"
        );
        assert_eq!(
            PromptError::MissingArg("scene".to_owned()).to_string(),
            "missing required argument: scene"
        );
        assert_eq!(
            PromptError::Load("bad yaml".to_owned()).to_string(),
            "failed to load prompt source: bad yaml"
        );
    }

    #[test]
    fn prompt_argument_defaults_description_and_required() {
        let arg: PromptArgumentSpec = serde_yaml_ng::from_str("name: frame_range").unwrap();

        assert_eq!(arg.name, "frame_range");
        assert_eq!(arg.description, None);
        assert!(!arg.required);
    }

    #[test]
    fn prompt_spec_accepts_minimal_yaml() {
        let spec: PromptSpec = serde_yaml_ng::from_str(
            r#"
name: inspect_scene
template: "Inspect {{scene}}."
"#,
        )
        .unwrap();

        assert_eq!(spec.name, "inspect_scene");
        assert_eq!(spec.description, None);
        assert!(spec.arguments.is_empty());
        assert_eq!(spec.template, "Inspect {{scene}}.");
    }

    #[test]
    fn prompts_spec_accepts_prompts_and_workflows() {
        let spec = PromptsSpec::from_yaml(
            r#"
prompts:
  - name: bake
    description: Bake a rig.
    arguments:
      - name: frame_range
        description: Frame range to bake.
        required: true
    template: "Bake {{frame_range}}."
workflows:
  - file: workflows/bake.yaml
    prompt_name: bake_workflow
"#,
        )
        .unwrap();

        assert_eq!(spec.prompts.len(), 1);
        assert_eq!(spec.prompts[0].name, "bake");
        assert_eq!(spec.prompts[0].arguments[0].name, "frame_range");
        assert!(spec.prompts[0].arguments[0].required);
        assert_eq!(spec.workflows.len(), 1);
        assert_eq!(spec.workflows[0].file, "workflows/bake.yaml");
        assert_eq!(
            spec.workflows[0].prompt_name.as_deref(),
            Some("bake_workflow")
        );
    }

    #[test]
    fn prompts_spec_defaults_missing_sections_to_empty() {
        let spec = PromptsSpec::from_yaml("{}").unwrap();

        assert!(spec.prompts.is_empty());
        assert!(spec.workflows.is_empty());
    }

    #[test]
    fn prompts_spec_round_trips_through_json() {
        let spec = PromptsSpec {
            prompts: vec![PromptSpec {
                name: "review".to_owned(),
                description: Some("Review scene".to_owned()),
                arguments: vec![PromptArgumentSpec {
                    name: "target".to_owned(),
                    description: None,
                    required: true,
                }],
                template: "Review {{target}}".to_owned(),
            }],
            workflows: vec![WorkflowPromptRef {
                file: "workflows/review.yaml".to_owned(),
                prompt_name: None,
            }],
        };

        let encoded = serde_json::to_string(&spec).unwrap();
        let decoded: PromptsSpec = serde_json::from_str(&encoded).unwrap();

        assert_eq!(decoded, spec);
    }
}
