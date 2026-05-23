use std::path::PathBuf;

use serde::{Deserialize, Serialize};

// ── WorkflowConfig ─────────────────────────────────────────────────────────

/// Workflow & scheduler configuration.
///
/// Captures the three opt-in switches that turn on the workflow
/// (`workflows_*` MCP tools, issue #348) and scheduler (issue #352)
/// subsystems. Both default to off so a pristine `McpHttpConfig`
/// boots the minimal surface and operators opt into the heavier
/// subsystems consciously.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WorkflowConfig {
    /// Enable the built-in `workflows_*` tools (issue #348).
    #[serde(default)]
    pub enable_workflows: bool,

    /// Enable the cron + webhook scheduler subsystem (issue #352).
    #[serde(default)]
    pub enable_scheduler: bool,

    /// Directory holding `*.schedules.yaml` files for the scheduler
    /// subsystem (issue #352).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub schedules_dir: Option<PathBuf>,
}
