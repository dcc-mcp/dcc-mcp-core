//! Built-in `workflows.*` MCP tool registrations.
//!
//! This module registers four tools into a [`ToolRegistry`]:
//!
//! | Tool name              | Status in skeleton | Behaviour                                    |
//! |------------------------|--------------------|----------------------------------------------|
//! | `workflows.run`        | stub               | validates spec → `NotImplemented` error      |
//! | `workflows.get_status` | stub               | `NotImplemented` error                       |
//! | `workflows.cancel`     | stub               | `NotImplemented` error                       |
//! | `workflows.lookup`     | **functional**     | read-only catalog summary                    |
//!
//! The tools are plain metadata entries — a real dispatcher (follow-up PR)
//! will hang Python or Rust handlers off these names via
//! `ToolDispatcher::register_handler`.

use dcc_mcp_actions::{ActionMeta, ActionRegistry};
use serde_json::json;

/// Stable wire-visible names for the `workflows.*` built-ins (public so
/// downstream crates can `use` them).
pub mod names {
    /// Start a new workflow run from an inline spec or a `{skill, name}` pair.
    pub const RUN: &str = "workflows.run";
    /// Poll aggregated workflow + child-job state.
    pub const GET_STATUS: &str = "workflows.get_status";
    /// Cancel an in-flight workflow.
    pub const CANCEL: &str = "workflows.cancel";
    /// Read-only catalog lookup — enumerate or filter known workflows.
    pub const LOOKUP: &str = "workflows.lookup";
}

/// Register all four `workflows.*` built-in tools on `registry`.
///
/// Safe to call multiple times on the same registry — the underlying
/// `DashMap` insert overwrites. Tool names are asserted against
/// `dcc_mcp_naming::validate_tool_name` at **compile-test time** via
/// [`crate::tests`] — this function never panics at runtime on the names
/// themselves.
pub fn register_builtin_workflow_tools(registry: &ActionRegistry) {
    registry.register_action(meta_run());
    registry.register_action(meta_get_status());
    registry.register_action(meta_cancel());
    registry.register_action(meta_lookup());
}

fn meta_run() -> ActionMeta {
    ActionMeta {
        name: names::RUN.to_string(),
        description: "Start a new workflow run. Accepts either an inline WorkflowSpec or a \
             {skill, name} pair resolved through the WorkflowCatalog. \
             (Step execution is pending a follow-up PR — see issue #348.)"
            .to_string(),
        category: "workflow".to_string(),
        dcc: "core".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        input_schema: json!({
            "type": "object",
            "oneOf": [
                {"required": ["spec"]},
                {"required": ["skill", "name"]},
            ],
            "properties": {
                "spec": {"type": "object", "description": "Inline WorkflowSpec."},
                "skill": {"type": "string"},
                "name": {"type": "string"},
                "inputs": {"type": "object"},
            },
        }),
        output_schema: not_implemented_output_schema(),
        ..Default::default()
    }
}

fn meta_get_status() -> ActionMeta {
    ActionMeta {
        name: names::GET_STATUS.to_string(),
        description: "Get aggregated status for a workflow run by workflow_id.".to_string(),
        category: "workflow".to_string(),
        dcc: "core".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        input_schema: json!({
            "type": "object",
            "required": ["workflow_id"],
            "properties": {"workflow_id": {"type": "string", "format": "uuid"}},
        }),
        output_schema: not_implemented_output_schema(),
        ..Default::default()
    }
}

fn meta_cancel() -> ActionMeta {
    ActionMeta {
        name: names::CANCEL.to_string(),
        description: "Cancel an in-flight workflow run by workflow_id (cascades to children)."
            .to_string(),
        category: "workflow".to_string(),
        dcc: "core".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        input_schema: json!({
            "type": "object",
            "required": ["workflow_id"],
            "properties": {"workflow_id": {"type": "string", "format": "uuid"}},
        }),
        output_schema: not_implemented_output_schema(),
        ..Default::default()
    }
}

fn meta_lookup() -> ActionMeta {
    ActionMeta {
        name: names::LOOKUP.to_string(),
        description: "List or search known workflows from the catalog. Read-only.".to_string(),
        category: "workflow".to_string(),
        dcc: "core".to_string(),
        version: env!("CARGO_PKG_VERSION").to_string(),
        input_schema: json!({
            "type": "object",
            "properties": {
                "skill": {"type": "string", "description": "Filter by owning skill name."},
                "name": {"type": "string", "description": "Exact workflow name match."},
                "query": {"type": "string", "description": "Free-text substring over name+description."},
            },
        }),
        output_schema: json!({
            "type": "object",
            "properties": {
                "workflows": {
                    "type": "array",
                    "items": {
                        "type": "object",
                        "required": ["name", "skill", "path"],
                        "properties": {
                            "name": {"type": "string"},
                            "skill": {"type": "string"},
                            "description": {"type": "string"},
                            "inputs": {"type": "object"},
                            "path": {"type": "string"},
                        },
                    },
                },
            },
        }),
        ..Default::default()
    }
}

/// Structured error shape returned by the three stub tools.
///
/// Kept as a const helper so downstream callers can rely on the exact key
/// set (`success: false`, `error: "not_implemented"`, `message: str`,
/// `issue: "#348"`) even before the execution PR lands.
#[must_use]
pub fn not_implemented_result(which: &str) -> serde_json::Value {
    json!({
        "success": false,
        "error": "not_implemented",
        "message": format!("{which}: step execution pending follow-up PR"),
        "issue": "#348",
    })
}

fn not_implemented_output_schema() -> serde_json::Value {
    json!({
        "type": "object",
        "required": ["success", "error"],
        "properties": {
            "success": {"type": "boolean"},
            "error": {"type": "string"},
            "message": {"type": "string"},
            "issue": {"type": "string"},
        },
    })
}
