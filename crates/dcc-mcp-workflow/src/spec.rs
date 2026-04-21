//! Declarative workflow types: [`WorkflowSpec`], [`Step`], [`StepKind`], status.

use std::collections::HashSet;
use std::str::FromStr;

use serde::{Deserialize, Serialize};
use uuid::Uuid;

use crate::error::{ValidationError, WorkflowError};

// ── Newtype ids ──────────────────────────────────────────────────────────

/// Unique identifier for a [`WorkflowSpec`] instance (runtime job).
///
/// Newtype over [`Uuid`]. Stable serde shape: plain string.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct WorkflowId(pub Uuid);

impl WorkflowId {
    /// Create a new random v4 id.
    #[must_use]
    pub fn new() -> Self {
        Self(Uuid::new_v4())
    }
}

impl Default for WorkflowId {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Display for WorkflowId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        self.0.fmt(f)
    }
}

impl FromStr for WorkflowId {
    type Err = uuid::Error;
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        Uuid::parse_str(s).map(Self)
    }
}

/// Identifier for a [`Step`] within a single [`WorkflowSpec`].
///
/// This is a **declared** id (authors choose it in YAML), not a UUID.
/// Uniqueness is enforced by [`WorkflowSpec::validate`].
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(transparent)]
pub struct StepId(pub String);

impl StepId {
    /// Borrow the underlying string.
    #[must_use]
    pub fn as_str(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Display for StepId {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl From<&str> for StepId {
    fn from(s: &str) -> Self {
        Self(s.to_string())
    }
}

impl From<String> for StepId {
    fn from(s: String) -> Self {
        Self(s)
    }
}

// ── Status ───────────────────────────────────────────────────────────────

/// Execution status of a [`WorkflowJob`](crate::job::WorkflowJob) or step.
///
/// `Interrupted` is the state reported by the recovery path when a running
/// workflow is killed mid-flight: finished steps stay finished, the first
/// unfinished step flips to `Interrupted`, and the workflow inherits that
/// state until a `workflows.resume` call re-schedules it. See #348.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WorkflowStatus {
    /// Submitted but no step has started yet.
    Pending,
    /// At least one step has started and no terminal state has been reached.
    Running,
    /// All steps completed successfully.
    Completed,
    /// A step failed and no error handler recovered it.
    Failed,
    /// The outer job was cancelled by the caller.
    Cancelled,
    /// The process crashed / was killed mid-run; resume candidate.
    Interrupted,
}

impl WorkflowStatus {
    /// Returns `true` if this status represents a terminal state.
    #[must_use]
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            Self::Completed | Self::Failed | Self::Cancelled | Self::Interrupted
        )
    }

    /// Lowercase string representation used in serde and Python bindings.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pending => "pending",
            Self::Running => "running",
            Self::Completed => "completed",
            Self::Failed => "failed",
            Self::Cancelled => "cancelled",
            Self::Interrupted => "interrupted",
        }
    }
}

impl std::fmt::Display for WorkflowStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

// ── Step kinds ───────────────────────────────────────────────────────────

/// Shape of a single step.
///
/// Uses an internal tag (`kind: ...` in YAML) so a `tool:` key alone is
/// implicit `kind: tool` (matches the example in #348). A missing `kind`
/// combined with a present `tool` field is normalised to [`StepKind::Tool`]
/// by the custom [`Step`] deserialiser.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum StepKind {
    /// Call one MCP tool on the local registry.
    Tool {
        /// Tool name — must pass [`dcc_mcp_naming::validate_tool_name`].
        tool: String,
        /// Inline arguments (template strings permitted, interpolation TBD).
        #[serde(default)]
        args: serde_json::Value,
    },

    /// Call one MCP tool on another DCC via the gateway.
    ToolRemote {
        /// Target DCC name (e.g. `"unreal"`).
        dcc: String,
        /// Remote tool name.
        tool: String,
        /// Inline arguments.
        #[serde(default)]
        args: serde_json::Value,
    },

    /// Iterate a JSONPath expression and run `steps` per item.
    Foreach {
        /// JSONPath expression into the workflow context.
        items: String,
        /// Binding name for the current item inside child step args.
        #[serde(default = "default_foreach_as")]
        r#as: String,
        /// Child subgraph executed per item.
        #[serde(default)]
        steps: Vec<Step>,
    },

    /// Run N children concurrently.
    Parallel {
        /// Child subgraph.
        #[serde(default)]
        steps: Vec<Step>,
    },

    /// Elicitation / approval gate. Blocks until accept / decline / cancel.
    Approve {
        /// Prompt shown to the approver (template string permitted).
        #[serde(default)]
        prompt: String,
    },

    /// Conditional branch — `on` is a JSONPath resolving to a boolean-ish
    /// value; `then` runs on truthy, `else` on falsy.
    Branch {
        /// JSONPath expression evaluated against the workflow context.
        on: String,
        /// Children to run on truthy.
        #[serde(default)]
        then: Vec<Step>,
        /// Children to run on falsy.
        #[serde(default, rename = "else")]
        else_steps: Vec<Step>,
    },
}

fn default_foreach_as() -> String {
    "item".to_string()
}

impl StepKind {
    /// Lowercase kind name, used in error messages.
    #[must_use]
    pub const fn kind_str(&self) -> &'static str {
        match self {
            Self::Tool { .. } => "tool",
            Self::ToolRemote { .. } => "tool_remote",
            Self::Foreach { .. } => "foreach",
            Self::Parallel { .. } => "parallel",
            Self::Approve { .. } => "approve",
            Self::Branch { .. } => "branch",
        }
    }
}

// ── Step ─────────────────────────────────────────────────────────────────

/// A single node in a [`WorkflowSpec`].
///
/// Custom deserialiser: supports the shorthand where `kind:` is omitted but
/// a top-level `tool:` key is present (treated as `kind: tool`).
#[derive(Debug, Clone, PartialEq)]
pub struct Step {
    /// Declared identifier, unique within the workflow.
    pub id: StepId,
    /// What this step does.
    pub kind: StepKind,
}

impl Serialize for Step {
    fn serialize<S: serde::Serializer>(&self, s: S) -> Result<S::Ok, S::Error> {
        use serde::ser::SerializeMap;
        // Flatten `{id, kind}` into a single map so output matches the input
        // shape (`id`, `kind`, plus kind-specific fields).
        let kind_value = serde_json::to_value(&self.kind).map_err(serde::ser::Error::custom)?;
        let obj = kind_value
            .as_object()
            .ok_or_else(|| serde::ser::Error::custom("StepKind did not serialize to an object"))?;

        let mut map = s.serialize_map(Some(obj.len() + 1))?;
        map.serialize_entry("id", &self.id)?;
        for (k, v) in obj {
            map.serialize_entry(k, v)?;
        }
        map.end()
    }
}

impl<'de> Deserialize<'de> for Step {
    fn deserialize<D: serde::Deserializer<'de>>(d: D) -> Result<Self, D::Error> {
        let mut value = serde_json::Value::deserialize(d)?;
        let obj = value
            .as_object_mut()
            .ok_or_else(|| serde::de::Error::custom("step must be a mapping"))?;

        // Extract id (required).
        let id = obj
            .remove("id")
            .ok_or_else(|| serde::de::Error::missing_field("id"))?
            .as_str()
            .ok_or_else(|| serde::de::Error::custom("step id must be a string"))?
            .to_string();

        // Normalise shorthand: `tool: foo` with no `kind:` → `kind: tool`.
        if !obj.contains_key("kind") && obj.contains_key("tool") && !obj.contains_key("dcc") {
            obj.insert("kind".into(), serde_json::Value::String("tool".into()));
        }

        let kind: StepKind = serde_json::from_value(serde_json::Value::Object(obj.clone()))
            .map_err(|e| {
                serde::de::Error::custom(format!("step {id:?}: failed to decode kind: {e}"))
            })?;

        Ok(Step {
            id: StepId(id),
            kind,
        })
    }
}

// ── Spec ─────────────────────────────────────────────────────────────────

/// Top-level workflow specification.
///
/// Parsed from YAML via [`Self::from_yaml`]. Validated via [`Self::validate`].
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct WorkflowSpec {
    /// Workflow name — used as the key in `workflows.run({skill, name})`.
    pub name: String,
    /// Human-readable description.
    #[serde(default)]
    pub description: String,
    /// JSON-schema-shaped input declaration (opaque in this skeleton).
    #[serde(default, skip_serializing_if = "serde_json::Value::is_null")]
    pub inputs: serde_json::Value,
    /// Ordered list of top-level steps.
    #[serde(default)]
    pub steps: Vec<Step>,
}

impl WorkflowSpec {
    /// Parse a workflow spec from a YAML document.
    ///
    /// # Errors
    ///
    /// Returns [`WorkflowError::Yaml`] on parse failure.
    pub fn from_yaml(s: &str) -> Result<Self, WorkflowError> {
        serde_yaml_ng::from_str::<Self>(s).map_err(|e| WorkflowError::Yaml(e.to_string()))
    }

    /// Validate structural invariants:
    ///
    /// - There is at least one step.
    /// - Every step id is non-empty and unique across the full tree.
    /// - Every `tool`/`tool_remote` tool name passes
    ///   [`dcc_mcp_naming::validate_tool_name`].
    /// - Every `branch.on` and `foreach.items` parses as a JSONPath
    ///   expression (via `jsonpath-rust`).
    ///
    /// # Errors
    ///
    /// Returns the first [`ValidationError`] encountered.
    pub fn validate(&self) -> Result<(), ValidationError> {
        if self.steps.is_empty() {
            return Err(ValidationError::NoSteps);
        }
        let mut seen = HashSet::new();
        for step in &self.steps {
            validate_step(step, &mut seen)?;
        }
        Ok(())
    }
}

fn validate_step(step: &Step, seen: &mut HashSet<String>) -> Result<(), ValidationError> {
    if step.id.0.is_empty() {
        return Err(ValidationError::EmptyStepId);
    }
    if !seen.insert(step.id.0.clone()) {
        return Err(ValidationError::DuplicateStepId(step.id.0.clone()));
    }

    match &step.kind {
        StepKind::Tool { tool, .. } | StepKind::ToolRemote { tool, .. } => {
            dcc_mcp_naming::validate_tool_name(tool).map_err(|e| {
                ValidationError::InvalidToolName {
                    step_id: step.id.0.clone(),
                    tool: tool.clone(),
                    reason: e.to_string(),
                }
            })?;
        }
        StepKind::Foreach { items, steps, .. } => {
            validate_jsonpath(&step.id.0, items)?;
            for child in steps {
                validate_step(child, seen)?;
            }
        }
        StepKind::Parallel { steps } => {
            for child in steps {
                validate_step(child, seen)?;
            }
        }
        StepKind::Branch {
            on,
            then,
            else_steps,
        } => {
            validate_jsonpath(&step.id.0, on)?;
            for child in then {
                validate_step(child, seen)?;
            }
            for child in else_steps {
                validate_step(child, seen)?;
            }
        }
        StepKind::Approve { .. } => {}
    }
    Ok(())
}

fn validate_jsonpath(step_id: &str, expr: &str) -> Result<(), ValidationError> {
    // `jsonpath-rust` 1.x exposes `parse_json_path` as its parse entry point.
    jsonpath_rust::parser::parse_json_path(expr).map_err(|e| ValidationError::InvalidJsonPath {
        step_id: step_id.to_string(),
        expr: expr.to_string(),
        reason: e.to_string(),
    })?;
    Ok(())
}
