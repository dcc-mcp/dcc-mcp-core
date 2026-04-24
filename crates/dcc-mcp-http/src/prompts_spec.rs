//! YAML spec types parsed from a skill's sibling `prompts.yaml`.

use serde::{Deserialize, Serialize};

/// Error type surfaced by [`crate::prompts::PromptRegistry::get`].
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
