//! Recall metadata for intelligent skill discovery (issue #1335).
//!
//! These types extend `SkillMetadata` and `ToolDeclaration` with structured,
//! optional fields the ranker, capability graph (#1336), lifecycle hooks
//! (#1337) and escape-hatch policy (#1325) can consume without changing the
//! wire shape of skills that do not opt in.
//!
//! Design rules:
//!
//! * Every field is optional.  Missing values are equivalent to "unknown".
//! * Types are pure data — no I/O, no validation logic here.  The validator
//!   in `dcc-mcp-skills` is the only place that turns "missing recommended
//!   field" into a warning.
//! * Enums use `#[serde(rename_all = "snake_case")]` so YAML, JSON, and the
//!   Rust label all agree.

use serde::{Deserialize, Serialize};

/// Top-level recall context for a skill or tool.
///
/// Tells discovery where this capability lives in the DCC universe so the
/// ranker can down-rank obviously-irrelevant candidates without loading full
/// schemas.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct RecallContext {
    /// Target application family — `"maya"`, `"blender"`, `"houdini"`, `"any"`.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub app_type: Option<String>,

    /// Domain bucket — `"modeling"`, `"rigging"`, `"rendering"`, `"io"`,
    /// `"diagnostics"`, …
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub domain: Option<String>,

    /// Workflow stage — adapter-defined, e.g. Maya uses `"bootstrap"`,
    /// `"scene"`, `"authoring"`, `"interchange"`, `"pipeline"`.
    #[serde(
        default,
        rename = "workflow_stage",
        alias = "workflow-stage",
        skip_serializing_if = "Option::is_none"
    )]
    pub workflow_stage: Option<String>,

    /// Task category — `"query"`, `"mutate"`, `"export"`, `"import"`,
    /// `"diagnose"`, …
    #[serde(
        default,
        rename = "task_category",
        alias = "task-category",
        skip_serializing_if = "Option::is_none"
    )]
    pub task_category: Option<String>,
}

impl RecallContext {
    /// Returns `true` when every field is unset.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.app_type.is_none()
            && self.domain.is_none()
            && self.workflow_stage.is_none()
            && self.task_category.is_none()
    }
}

/// A single precondition that must hold for a skill or tool to run safely.
///
/// Open-ended via the `Other` variant so adapters can express studio-specific
/// requirements without core needing a new variant per DCC.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Precondition {
    /// Specific application + optional version constraint, e.g. Maya 2024+.
    Software {
        name: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        version: Option<String>,
    },
    /// Loaded plugin or module the host must have available.
    Plugin { name: String },
    /// Required selection state in the host.
    Selection {
        kind: String,
        #[serde(default, skip_serializing_if = "Option::is_none")]
        min: Option<u32>,
    },
    /// Scene state predicate (`"has_active_camera"`, `"in_object_mode"`, …).
    SceneState { predicate: String },
    /// Adapter capability tag (mirrors `ToolDeclaration::required_capabilities`).
    Capability { tag: String },
    /// Free-form fallback for conditions the modelled variants cannot express.
    Other { description: String },
}

/// Side-effect descriptor — what does this skill or tool change?
///
/// The booleans surface as agent-facing safety hints; `targets` is the
/// machine-readable list of artefact/object tags fed into the capability
/// graph (#1336) — e.g. `["scene_node", "file:fbx"]`.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct SideEffects {
    #[serde(default, skip_serializing_if = "is_false")]
    pub creates: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub modifies: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub deletes: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub exports: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub imports: bool,
    #[serde(
        default,
        rename = "ui_mutation",
        alias = "ui-mutation",
        skip_serializing_if = "is_false"
    )]
    pub ui_mutation: bool,
    #[serde(
        default,
        rename = "file_output",
        alias = "file-output",
        skip_serializing_if = "is_false"
    )]
    pub file_output: bool,
    #[serde(default, skip_serializing_if = "is_false")]
    pub render: bool,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub targets: Vec<String>,
}

impl SideEffects {
    /// Returns `true` when every flag is `false` and `targets` is empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        !(self.creates
            || self.modifies
            || self.deletes
            || self.exports
            || self.imports
            || self.ui_mutation
            || self.file_output
            || self.render)
            && self.targets.is_empty()
    }
}

fn is_false(b: &bool) -> bool {
    !*b
}

fn is_zero_u64(v: &u64) -> bool {
    *v == 0
}

/// Tool semantic role — drives ranking and safety surfaces (issue #1325).
///
/// `EscapeHatch` is what generic-scripting tools (`execute_python`, host
/// script eval, MaxScript-style execution) declare so the gateway can demote
/// them in search results unless the agent explicitly asks for scripting.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ToolRole {
    /// Pure read tool — no observable state change.
    ReadOnly,
    /// Normal action — mutates state but recoverable through undo / re-run.
    Action,
    /// Destructive mutation — undo may not recover.
    Destructive,
    /// Generic scripting / shell / eval escape hatch.  Only ranked when no
    /// typed skill covers the query.
    EscapeHatch,
    /// Debug-only tool — hidden from production discovery unless explicitly
    /// requested.
    DebugOnly,
}

impl ToolRole {
    /// Short label used in JSON, logs, and Python `__str__`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::ReadOnly => "read_only",
            Self::Action => "action",
            Self::Destructive => "destructive",
            Self::EscapeHatch => "escape_hatch",
            Self::DebugOnly => "debug_only",
        }
    }

    /// Whether this role should be treated as an explicit fallback path
    /// rather than a normal first-class capability.
    #[must_use]
    pub fn is_escape_hatch(self) -> bool {
        matches!(self, Self::EscapeHatch)
    }
}

/// Coarse risk classification surfaced to agents and admins.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum RiskLevel {
    /// Read-only / sandboxed operation.
    Low,
    /// Mutates host state but bounded.
    Medium,
    /// Destructive or irreversible without explicit acknowledgement.
    High,
    /// Host scripting / shell execution — must be paired with
    /// `ToolRole::EscapeHatch`.
    HostScriptExecution,
}

impl RiskLevel {
    /// Short label used in JSON, logs, and Python `__str__`.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Low => "low",
            Self::Medium => "medium",
            Self::High => "high",
            Self::HostScriptExecution => "host_script_execution",
        }
    }
}

/// Aggregate success metrics for a skill or tool.
///
/// Populated at runtime by the ranker / memory layer (#1334) — never written
/// by hand into SKILL.md.  Stored alongside the static metadata so the same
/// type round-trips through the public `SkillMetadata` JSON when an admin
/// surface chooses to expose it.
#[derive(Debug, Clone, Default, PartialEq, Serialize, Deserialize)]
pub struct SuccessMetrics {
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub success_count: u64,
    #[serde(default, skip_serializing_if = "is_zero_u64")]
    pub failure_count: u64,
    #[serde(
        default,
        rename = "last_used_unix_secs",
        alias = "last-used-unix-secs",
        skip_serializing_if = "Option::is_none"
    )]
    pub last_used_unix_secs: Option<i64>,
    #[serde(
        default,
        rename = "recent_failure_kinds",
        alias = "recent-failure-kinds",
        skip_serializing_if = "Vec::is_empty"
    )]
    pub recent_failure_kinds: Vec<String>,
    #[serde(
        default,
        rename = "mean_selected_rank",
        alias = "mean-selected-rank",
        skip_serializing_if = "Option::is_none"
    )]
    pub mean_selected_rank: Option<f32>,
}

impl SuccessMetrics {
    /// Returns `true` when no observations have been recorded.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.success_count == 0
            && self.failure_count == 0
            && self.last_used_unix_secs.is_none()
            && self.recent_failure_kinds.is_empty()
            && self.mean_selected_rank.is_none()
    }

    /// Empirical success rate over observed runs, or `None` if no runs yet.
    #[must_use]
    pub fn success_rate(&self) -> Option<f32> {
        let total = self.success_count + self.failure_count;
        if total == 0 {
            None
        } else {
            Some(self.success_count as f32 / total as f32)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn recall_context_serde_omits_unset_fields() {
        let ctx = RecallContext {
            app_type: Some("maya".into()),
            ..Default::default()
        };
        let json = serde_json::to_string(&ctx).unwrap();
        assert_eq!(json, r#"{"app_type":"maya"}"#);
        let back: RecallContext = serde_json::from_str(&json).unwrap();
        assert_eq!(back, ctx);
    }

    #[test]
    fn precondition_software_serde_roundtrip() {
        let p = Precondition::Software {
            name: "maya".into(),
            version: Some(">=2024".into()),
        };
        let json = serde_json::to_string(&p).unwrap();
        let back: Precondition = serde_json::from_str(&json).unwrap();
        assert_eq!(back, p);
    }

    #[test]
    fn side_effects_default_is_empty() {
        let s = SideEffects::default();
        assert!(s.is_empty());
        let json = serde_json::to_string(&s).unwrap();
        assert_eq!(json, "{}");
    }

    #[test]
    fn tool_role_serde_snake_case() {
        let role = ToolRole::EscapeHatch;
        let json = serde_json::to_string(&role).unwrap();
        assert_eq!(json, "\"escape_hatch\"");
        assert_eq!(role.label(), "escape_hatch");
        assert!(role.is_escape_hatch());
    }

    #[test]
    fn risk_level_host_script_label() {
        assert_eq!(
            RiskLevel::HostScriptExecution.label(),
            "host_script_execution"
        );
    }

    #[test]
    fn success_metrics_rate_is_none_when_unused() {
        assert_eq!(SuccessMetrics::default().success_rate(), None);
    }

    #[test]
    fn success_metrics_rate_computes_correctly() {
        let m = SuccessMetrics {
            success_count: 3,
            failure_count: 1,
            ..Default::default()
        };
        assert!((m.success_rate().unwrap() - 0.75).abs() < f32::EPSILON);
    }
}
