//! Adapter-owned job discovery and poll registration.

use serde_json::{Value, json};

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_models::ExecutionMode;

use crate::server_state::ServerState;

const JOB_STATUSES: &[&str] = &[
    "pending",
    "running",
    "completed",
    "failed",
    "cancelled",
    "interrupted",
];
const TERMINAL_JOB_STATUSES: &[&str] = &["completed", "failed", "cancelled", "interrupted"];

pub(crate) fn attach_direct_adapter_job_contract(
    state: &ServerState,
    action: &ToolMeta,
    output: &mut Value,
) {
    let Some(job) = direct_adapter_job(output) else {
        return;
    };
    let poll_tool = adapter_poll_tool(state, action);
    let mut descriptor = output
        .get("adapter_job")
        .cloned()
        .filter(Value::is_object)
        .unwrap_or_else(|| json!({}));
    let descriptor_object = descriptor
        .as_object_mut()
        .expect("adapter job descriptor was initialized as an object");
    descriptor_object.insert("job_id".to_string(), Value::String(job.job_id.clone()));
    descriptor_object.insert("owner".to_string(), Value::String("adapter".to_string()));
    descriptor_object.insert(
        "identity_source".to_string(),
        Value::String(job.identity_source.to_string()),
    );
    descriptor_object.insert("status".to_string(), Value::String(job.status.clone()));
    descriptor_object.insert(
        "terminal".to_string(),
        Value::Bool(TERMINAL_JOB_STATUSES.contains(&job.status.as_str())),
    );
    descriptor_object.insert(
        "poll_contract".to_string(),
        json!({
            "registered": poll_tool.is_some(),
            "reason": poll_tool.is_none().then_some("safe_poll_tool_not_declared"),
        }),
    );
    let cancellation = descriptor_object
        .entry("cancellation".to_string())
        .or_insert_with(|| json!({}));
    if !cancellation.is_object() {
        *cancellation = json!({});
    }
    let cancellation = cancellation
        .as_object_mut()
        .expect("adapter cancellation descriptor was initialized as an object");
    cancellation.insert("owner".to_string(), Value::String("adapter".to_string()));
    cancellation.insert("inherits_core_cancellation".to_string(), Value::Bool(false));
    descriptor_object.remove("poll");
    if let Some(progress) = job.progress {
        descriptor_object.insert("progress".to_string(), progress);
    }
    if let Some(tool) = poll_tool {
        descriptor_object.insert(
            "poll".to_string(),
            json!({
                "owner": "adapter",
                "tool": tool,
                "arguments": {"job_id": job.job_id},
            }),
        );
    }
    let Some(object) = output.as_object_mut() else {
        return;
    };
    object.insert(
        "job_id_owner".to_string(),
        Value::String("adapter".to_string()),
    );
    object.insert(
        "adapter_job_id".to_string(),
        Value::String(job.job_id.clone()),
    );
    object.insert("adapter_job".to_string(), descriptor);
}

pub(crate) fn adapter_poll_tool(state: &ServerState, action: &ToolMeta) -> Option<String> {
    if accepts_adapter_job_id(action) {
        return Some(action.name.clone());
    }
    action
        .next_tools
        .on_success
        .iter()
        .filter_map(|declared| resolve_follow_up(state, action, declared))
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
    meta.execution == ExecutionMode::Sync
        && meta.annotations.read_only_hint == Some(true)
        && meta.annotations.idempotent_hint == Some(true)
        && meta
            .input_schema
            .pointer("/properties/job_id/type")
            .and_then(Value::as_str)
            == Some("string")
        && meta
            .input_schema
            .get("required")
            .and_then(Value::as_array)
            .is_some_and(|required| required.len() == 1 && required[0].as_str() == Some("job_id"))
}

struct DirectAdapterJob {
    job_id: String,
    identity_source: &'static str,
    status: String,
    progress: Option<Value>,
}

fn direct_adapter_job(output: &Value) -> Option<DirectAdapterJob> {
    [
        (
            "/adapter_job/job_id",
            "/adapter_job/status",
            "/adapter_job/progress",
        ),
        (
            "/context/adapter_job_id",
            "/context/status",
            "/context/progress",
        ),
        ("/context/job_id", "/context/status", "/context/progress"),
        ("/job_id", "/status", "/progress"),
    ]
    .into_iter()
    .find_map(|(job_path, status_path, progress_path)| {
        let job_id = output.pointer(job_path)?.as_str()?.trim();
        let status = output.pointer(status_path)?.as_str()?.trim();
        if job_id.is_empty() || !JOB_STATUSES.contains(&status) {
            return None;
        }
        Some(DirectAdapterJob {
            job_id: job_id.to_string(),
            identity_source: match job_path {
                "/adapter_job/job_id" => "result.adapter_job.job_id",
                "/context/adapter_job_id" => "result.context.adapter_job_id",
                "/context/job_id" => "result.context.job_id",
                _ => "result.job_id",
            },
            status: status.to_string(),
            progress: output.pointer(progress_path).cloned(),
        })
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_models::SkillToolAnnotations;

    fn poll_meta() -> ToolMeta {
        ToolMeta {
            name: "render__get_job".to_string(),
            input_schema: json!({
                "type": "object",
                "properties": {"job_id": {"type": "string"}},
                "required": ["job_id"],
            }),
            annotations: SkillToolAnnotations {
                read_only_hint: Some(true),
                idempotent_hint: Some(true),
                ..Default::default()
            },
            ..Default::default()
        }
    }

    #[test]
    fn adapter_poll_contract_requires_sole_required_string_job_id() {
        let safe = poll_meta();
        assert!(accepts_adapter_job_id(&safe));

        let mut asynchronous = safe.clone();
        asynchronous.execution = ExecutionMode::Async;
        assert!(!accepts_adapter_job_id(&asynchronous));

        let mut writable = safe.clone();
        writable.annotations.read_only_hint = Some(false);
        assert!(!accepts_adapter_job_id(&writable));

        let mut non_idempotent = safe.clone();
        non_idempotent.annotations.idempotent_hint = Some(false);
        assert!(!accepts_adapter_job_id(&non_idempotent));

        let mut untyped = safe.clone();
        untyped.input_schema["properties"]["job_id"]["type"] = json!("integer");
        assert!(!accepts_adapter_job_id(&untyped));

        let mut optional = safe;
        optional.input_schema["required"] = json!([]);
        assert!(!accepts_adapter_job_id(&optional));

        let mut extra_required = poll_meta();
        extra_required.input_schema["properties"]["include_details"] = json!({"type": "boolean"});
        extra_required.input_schema["required"] = json!(["job_id", "include_details"]);
        assert!(!accepts_adapter_job_id(&extra_required));
    }
}
