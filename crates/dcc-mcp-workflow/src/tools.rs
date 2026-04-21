//! Built-in `workflows.*` MCP tool registrations.
//!
//! This module exposes four tools for the workflow primitive (issue #348):
//!
//! | Tool name              | Status             | Behaviour                                     |
//! |------------------------|--------------------|-----------------------------------------------|
//! | `workflows.run`        | functional         | Starts a run via [`WorkflowHost`]             |
//! | `workflows.get_status` | functional         | Queries the run registry for terminal status  |
//! | `workflows.cancel`     | functional         | Flips the run's cancellation token            |
//! | `workflows.lookup`     | read-only catalog  | Lists / searches the [`WorkflowCatalog`]      |
//!
//! Two helpers are provided:
//!
//! - [`register_builtin_workflow_tools`] — register tool **metadata** in a
//!   [`ActionRegistry`]. Safe to call before the executor exists.
//! - [`register_workflow_handlers`] — register functional handlers in a
//!   [`ActionDispatcher`] bound to a [`WorkflowHost`]. Call after the
//!   executor is wired.
//!
//! The metadata and handler registration are intentionally split so
//! `tools/list` can advertise the workflow surface even before a host is
//! built (e.g. during early MCP capability negotiation).

use dcc_mcp_actions::dispatcher::ActionDispatcher;
use dcc_mcp_actions::{ActionMeta, ActionRegistry};
use dcc_mcp_models::ToolAnnotations;
use serde_json::{Value, json};

use crate::host::{WorkflowHost, cancel_handler, get_status_handler, run_handler};

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

/// Register **functional** handlers for the three mutating workflow tools
/// against a [`ActionDispatcher`] bound to a shared [`WorkflowHost`].
///
/// `workflows.lookup` is intentionally **not** wired here — it is a pure
/// catalog read whose handler depends on having a [`crate::WorkflowCatalog`]
/// in scope, which lives at the server-layer boundary.
///
/// Call after the dispatcher's registry has been populated via
/// [`register_builtin_workflow_tools`]. Invocations from a Tokio runtime
/// will succeed; invocations outside a runtime will surface as a
/// `workflow start failed: ...` error on the `run` handler. Status/cancel
/// handlers are safe to call from any thread.
pub fn register_workflow_handlers(dispatcher: &ActionDispatcher, host: &WorkflowHost) {
    let h = host.clone();
    dispatcher.register_handler(names::RUN, move |args| run_handler(&h, args));
    let h = host.clone();
    dispatcher.register_handler(names::GET_STATUS, move |args| get_status_handler(&h, args));
    let h = host.clone();
    dispatcher.register_handler(names::CANCEL, move |args| cancel_handler(&h, args));
}

fn meta_run() -> ActionMeta {
    ActionMeta {
        name: names::RUN.to_string(),
        description:
            "Start a new workflow run. Accepts either an inline WorkflowSpec (YAML string \
             or JSON object) or a {skill, name} pair resolved through the WorkflowCatalog. \
             Returns { workflow_id, root_job_id, status: \"pending\" } immediately; \
             subscribe to `$/dcc.workflowUpdated` for progress."
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
                "spec": {
                    "description": "Inline WorkflowSpec (YAML string or JSON object).",
                    "oneOf": [
                        {"type": "string"},
                        {"type": "object"},
                    ],
                },
                "skill": {"type": "string"},
                "name": {"type": "string"},
                "inputs": {"type": "object"},
                "parent_job_id": {"type": "string", "format": "uuid"},
            },
        }),
        output_schema: run_output_schema(),
        // Workflows usually mutate scenes → destructive.
        annotations: ToolAnnotations {
            destructive_hint: Some(true),
            read_only_hint: Some(false),
            idempotent_hint: Some(false),
            open_world_hint: Some(true),
            ..Default::default()
        },
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
        output_schema: status_output_schema(),
        annotations: ToolAnnotations {
            destructive_hint: Some(false),
            read_only_hint: Some(true),
            idempotent_hint: Some(true),
            ..Default::default()
        },
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
        output_schema: cancel_output_schema(),
        annotations: ToolAnnotations {
            destructive_hint: Some(true),
            idempotent_hint: Some(true),
            read_only_hint: Some(false),
            ..Default::default()
        },
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
        annotations: ToolAnnotations {
            read_only_hint: Some(true),
            destructive_hint: Some(false),
            idempotent_hint: Some(true),
            ..Default::default()
        },
        ..Default::default()
    }
}

fn run_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["workflow_id", "root_job_id", "status"],
        "properties": {
            "workflow_id": {"type": "string", "format": "uuid"},
            "root_job_id": {"type": "string", "format": "uuid"},
            "status": {"type": "string", "enum": ["pending", "running"]},
        },
    })
}

fn status_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["workflow_id", "root_job_id", "status", "terminal"],
        "properties": {
            "workflow_id": {"type": "string", "format": "uuid"},
            "root_job_id": {"type": "string", "format": "uuid"},
            "status": {
                "type": "string",
                "enum": ["pending", "running", "completed", "failed", "cancelled", "interrupted"],
            },
            "terminal": {"type": "boolean"},
        },
    })
}

fn cancel_output_schema() -> Value {
    json!({
        "type": "object",
        "required": ["workflow_id", "cancelled"],
        "properties": {
            "workflow_id": {"type": "string", "format": "uuid"},
            "cancelled": {
                "type": "boolean",
                "description": "False when the id was unknown (already finished / never existed).",
            },
        },
    })
}

/// Legacy pre-host stub error payload. Kept for tests that assert on the
/// shape produced before the execution PR landed. New code should rely on
/// the structured outputs defined above.
#[must_use]
#[doc(hidden)]
pub fn not_implemented_result(which: &str) -> serde_json::Value {
    json!({
        "success": false,
        "error": "not_implemented",
        "message": format!("{which}: no WorkflowHost wired — handler registration pending"),
        "issue": "#348",
    })
}
