//! Async job status MCP handlers.

use chrono;
use serde_json::{Value, json};

use dcc_mcp_job::job::{Job, JobStatus};
use dcc_mcp_jsonrpc::{CallToolResult, ToolContent};

use crate::server_state::ServerState;

pub(in crate::rmcp_tool_call_dispatch) fn compute_job_timestamps(
    job: &Job,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    let started_at = match job.status {
        JobStatus::Pending => None,
        _ => Some(job.updated_at),
    };
    let completed_at = if job.status.is_terminal() {
        Some(job.updated_at)
    } else {
        None
    };
    (started_at, completed_at)
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
            "jobs.get_status received include_logs=true — no-op, JobManager does not capture logs"
        );
    }

    let Some(entry) = state.jobs.get(job_id) else {
        return CallToolResult::error(format!("No job found with id '{job_id}'"));
    };
    let job = entry.read();

    let (started_at, completed_at) = compute_job_timestamps(&job);
    let mut envelope = serde_json::Map::new();
    envelope.insert("job_id".into(), Value::String(job.id.clone()));
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
