//! Async job status MCP handlers.

use chrono;
use serde_json::{Value, json};

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_job::job::Job;
use dcc_mcp_jsonrpc::{CallToolResult, ToolContent};
use dcc_mcp_models::linked_adapter_job_from_result;

use crate::server_state::ServerState;

pub(in crate::rmcp_tool_call_dispatch) fn compute_job_timestamps(
    job: &Job,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    (job.started_at, job.completed_at)
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_jobs_get_status(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let job_id = arguments
        .get("job_id")
        .and_then(Value::as_str)
        .unwrap_or("")
        .trim();
    if job_id.is_empty() {
        return CallToolResult::error("Missing required parameter: job_id".to_string());
    }
    let include_logs = arguments
        .get("include_logs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_result = arguments
        .get("include_result")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    if include_logs {
        tracing::debug!(
            job_id = %job_id,
            "jobs_get_status received include_logs=true — no-op, JobManager does not capture logs"
        );
    }

    let Some(entry) = state.jobs.get(job_id) else {
        return CallToolResult::error(format!("No job found with id '{job_id}'"));
    };
    let job = entry.read();

    let (started_at, completed_at) = compute_job_timestamps(&job);
    let mut envelope = serde_json::Map::new();
    envelope.insert("job_id".into(), Value::String(job.id.clone()));
    envelope.insert("core_job_id".into(), Value::String(job.id.clone()));
    envelope.insert("job_id_owner".into(), Value::String("core".into()));
    envelope.insert(
        "core_poll".into(),
        json!({
            "owner": "core",
            "tool": "jobs_get_status",
            "arguments": {"job_id": job.id, "include_result": true},
        }),
    );
    envelope.insert(
        "parent_job_id".into(),
        match &job.parent_job_id {
            Some(p) => Value::String(p.clone()),
            None => Value::Null,
        },
    );
    envelope.insert("tool".into(), Value::String(job.tool_name.clone()));
    envelope.insert(
        "status".into(),
        serde_json::to_value(job.status).unwrap_or(Value::Null),
    );
    envelope.insert(
        "created_at".into(),
        Value::String(job.created_at.to_rfc3339()),
    );
    envelope.insert(
        "started_at".into(),
        started_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "completed_at".into(),
        completed_at
            .map(|t| Value::String(t.to_rfc3339()))
            .unwrap_or(Value::Null),
    );
    envelope.insert(
        "updated_at".into(),
        Value::String(job.updated_at.to_rfc3339()),
    );
    envelope.insert(
        "progress".into(),
        serde_json::to_value(&job.progress).unwrap_or(Value::Null),
    );
    envelope.insert(
        "error".into(),
        match &job.error {
            Some(e) => Value::String(e.clone()),
            None => Value::Null,
        },
    );
    if include_result
        && job.status.is_terminal()
        && let Some(ref r) = job.result
    {
        envelope.insert("result".into(), r.clone());
    }
    if job.status.is_terminal()
        && let Some(adapter_job) = job
            .result
            .as_ref()
            .and_then(|result| linked_adapter_job_from_result(result, &job.id))
    {
        let poll_tool = adapter_poll_tool(state, &job);
        let hint = match poll_tool.as_deref() {
            Some(tool) => format!(
                "Call adapter-owned status tool {tool} with adapter_job_id; do not pass this id to jobs_get_status."
            ),
            None => "Discover the adapter's typed status tool and pass adapter_job_id; do not pass this id to jobs_get_status."
                .to_string(),
        };
        let mut descriptor = json!({
            "job_id": adapter_job.job_id,
            "owner": "adapter",
            "identity_source": adapter_job.source,
            "core_job_id": job.id,
            "cancellation": {
                "owner": "adapter",
                "inherits_core_cancellation": false,
            },
            "hint": hint,
        });
        if let Some(tool) = poll_tool {
            descriptor["poll"] = json!({
                "owner": "adapter",
                "tool": tool,
                "arguments": {"job_id": adapter_job.job_id},
            });
        }
        envelope.insert("adapter_job_id".into(), Value::String(adapter_job.job_id));
        envelope.insert("adapter_job".into(), descriptor);
    }
    drop(job);

    let envelope_value = Value::Object(envelope);
    let text = serde_json::to_string(&envelope_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(envelope_value),
        is_error: false,
        meta: None,
    }
}

fn adapter_poll_tool(state: &ServerState, job: &Job) -> Option<String> {
    let action = state.registry.get_action(&job.tool_name, None)?;
    action
        .next_tools
        .on_success
        .iter()
        .filter_map(|declared| resolve_follow_up(state, &action, declared))
        .find(|(_, meta)| accepts_adapter_job_id(meta))
        .map(|(name, _)| name)
}

fn resolve_follow_up(
    state: &ServerState,
    action: &ToolMeta,
    declared: &str,
) -> Option<(String, ToolMeta)> {
    if let Some(meta) = state.registry.get_action(declared, None) {
        return Some((declared.to_string(), meta));
    }
    let (prefix, _) = action.name.rsplit_once("__")?;
    let qualified = format!("{prefix}__{declared}");
    state
        .registry
        .get_action(&qualified, None)
        .map(|meta| (qualified, meta))
}

fn accepts_adapter_job_id(meta: &ToolMeta) -> bool {
    meta.annotations.read_only_hint == Some(true)
        && meta
            .input_schema
            .get("properties")
            .and_then(Value::as_object)
            .is_some_and(|properties| properties.contains_key("job_id"))
        && meta
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| required.iter().any(|name| name.as_str() == Some("job_id")))
}

pub(in crate::rmcp_tool_call_dispatch) fn handle_jobs_cleanup(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let older_than_hours = arguments
        .get("older_than_hours")
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let removed = state.jobs.cleanup_older_than_hours(older_than_hours);
    let envelope = json!({
        "removed": removed,
        "older_than_hours": older_than_hours,
    });
    let text = serde_json::to_string(&envelope).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(envelope),
        is_error: false,
        meta: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Arc;

    use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
    use dcc_mcp_job::job::{JobManager, JobProgress};
    use dcc_mcp_models::{NextTools, ToolAnnotations};
    use dcc_mcp_skills::SkillCatalog;

    #[test]
    fn reported_start_timestamp_does_not_move_when_job_completes() {
        let jobs = JobManager::new();
        let handle = jobs.create("render.sequence");
        let id = handle.read().id.clone();
        jobs.start(&id).unwrap();
        let started_at = compute_job_timestamps(&handle.read()).0;

        jobs.update_progress(
            &id,
            JobProgress {
                current: 1,
                total: 2,
                message: None,
            },
        )
        .unwrap();
        jobs.complete(&id, json!({"ok": true})).unwrap();

        let (reported_start, reported_completion) = compute_job_timestamps(&handle.read());
        assert_eq!(reported_start, started_at);
        assert_eq!(reported_completion, Some(handle.read().updated_at));
    }

    #[test]
    fn terminal_core_job_exposes_declared_adapter_poll_contract() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(ToolMeta {
            name: "houdini_render__flipbook".into(),
            next_tools: NextTools {
                on_success: vec!["get_flipbook_job".into()],
                ..Default::default()
            },
            ..Default::default()
        });
        registry.register_action(ToolMeta {
            name: "houdini_render__get_flipbook_job".into(),
            input_schema: json!({
                "type": "object",
                "properties": {"job_id": {"type": "string"}},
                "required": ["job_id"],
            }),
            annotations: ToolAnnotations {
                read_only_hint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        });
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let jobs = Arc::new(JobManager::new());
        let handle = jobs.create("houdini_render__flipbook");
        let core_job_id = handle.read().id.clone();
        jobs.start(&core_job_id).unwrap();
        jobs.complete(
            &core_job_id,
            json!({
                "context": {"job_id": "flipbook-f0631aa83e07"},
                "progress": {"current": 96, "total": 96},
            }),
        )
        .unwrap();
        let state = ServerState::builder(registry, dispatcher, catalog)
            .with_jobs(jobs)
            .build();

        let result = handle_jobs_get_status(
            &state,
            &json!({"job_id": core_job_id, "include_result": true}),
        );
        let payload = result.structured_content.unwrap();

        assert_eq!(payload["job_id_owner"], "core");
        assert_eq!(payload["core_job_id"], core_job_id);
        assert_eq!(payload["adapter_job_id"], "flipbook-f0631aa83e07");
        assert_eq!(payload["adapter_job"]["owner"], "adapter");
        assert_eq!(
            payload["adapter_job"]["poll"],
            json!({
                "owner": "adapter",
                "tool": "houdini_render__get_flipbook_job",
                "arguments": {"job_id": "flipbook-f0631aa83e07"},
            })
        );
        assert_eq!(
            payload["adapter_job"]["cancellation"],
            json!({"owner": "adapter", "inherits_core_cancellation": false})
        );
    }
}
