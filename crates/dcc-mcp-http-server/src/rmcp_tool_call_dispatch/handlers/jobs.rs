//! Async job status MCP handlers.

use chrono;
use serde_json::{Value, json};

use dcc_mcp_job::job::Job;
use dcc_mcp_job::poller::{
    output_counter_from_result, output_counter_from_target, reconcile_progress,
};
use dcc_mcp_jsonrpc::{CallToolResult, ToolContent};
use dcc_mcp_models::linked_adapter_job_from_result;

use crate::rmcp_tool_call_dispatch::adapter_jobs::ensure_poll_contract;
use crate::server_state::ServerState;

pub(in crate::rmcp_tool_call_dispatch) fn compute_job_timestamps(
    job: &Job,
) -> (
    Option<chrono::DateTime<chrono::Utc>>,
    Option<chrono::DateTime<chrono::Utc>>,
) {
    (job.started_at, job.completed_at)
}

fn project_job_error(error: &str) -> Value {
    let Ok(value) = serde_json::from_str::<Value>(error) else {
        return Value::String(error.to_owned());
    };
    let recognized = value
        .as_object()
        .and_then(|object| {
            object
                .get("layer")
                .and_then(Value::as_str)
                .zip(object.get("code").and_then(Value::as_str))
        })
        .is_some_and(|(layer, code)| layer == "instance" && code.starts_with("SPLIT_PHASE_"));
    if recognized {
        value
    } else {
        Value::String(error.to_owned())
    }
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
    // Counters are reconciled against authoritative state at read time: a job
    // that declares where its outputs land never reports a cached zero while
    // frames are already on disk (issue #2262).
    //
    // A terminal job declares its output directory in `result`. A job that is
    // still running has no result yet, so it falls back to the output target
    // captured from its launch arguments — that is the case #2262 describes.
    let observed_outputs = job
        .result
        .as_ref()
        .and_then(output_counter_from_result)
        .or_else(|| job.output.as_ref().map(output_counter_from_target))
        .and_then(|counter| counter.count());
    let progress = reconcile_progress(job.progress.as_ref(), observed_outputs)
        .unwrap_or_else(|| serde_json::to_value(&job.progress).unwrap_or(Value::Null));
    envelope.insert("progress".into(), progress);
    envelope.insert(
        "error".into(),
        match &job.error {
            Some(e) => project_job_error(e),
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
        // Resolve through the poller so the contract advertised here is one
        // `jobs_poll_contract` can also resolve (issue #2262).
        let launch_result = job.result.clone().unwrap_or(Value::Null);
        let poll_contract = state
            .registry
            .get_action(&job.tool_name, None)
            .and_then(|action| ensure_poll_contract(state, &action, &launch_result, None));
        let poll_tool = poll_contract.as_ref().map(|contract| contract.tool.clone());
        let poll_registered = poll_tool.is_some();
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
            "poll_contract": {
                "registered": poll_registered,
                "reason": (!poll_registered).then_some("safe_poll_tool_not_declared"),
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

/// Report how a job type is polled by the unified poller (issue #2262).
///
/// With `job_type` this answers "can `--wait` reach a terminal state for this
/// tool?" — `registered: false` names the reason instead of letting the wait
/// return early. Without it, every registered job type is listed so operators
/// can see which async tools are actually tracked.
pub(in crate::rmcp_tool_call_dispatch) fn handle_jobs_poll_contract(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let job_type = arguments
        .get("job_type")
        .and_then(Value::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty());

    let envelope = match job_type {
        Some(job_type) => state.poll_registry.contract_value(Some(job_type)),
        None => {
            let registered_types = state
                .poll_registry
                .list()
                .into_iter()
                .map(|registration| {
                    state
                        .poll_registry
                        .contract_value(Some(&registration.job_type))
                })
                .collect::<Vec<_>>();
            json!({
                "count": registered_types.len(),
                "registered_types": registered_types,
            })
        }
    };
    let text = serde_json::to_string(&envelope).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(envelope),
        is_error: false,
        meta: None,
    }
}

pub(in crate::rmcp_tool_call_dispatch) async fn handle_jobs_cleanup(
    state: &ServerState,
    arguments: &Value,
) -> CallToolResult {
    let older_than_hours = arguments
        .get("older_than_hours")
        .and_then(Value::as_u64)
        .unwrap_or(24);
    let jobs = state.jobs.clone();
    let cleanup_timeout = std::time::Duration::from_millis(250);
    let removed = match tokio::time::timeout(
        cleanup_timeout,
        tokio::task::spawn_blocking(move || {
            jobs.cleanup_older_than_hours_blocking_with_timeout(older_than_hours, cleanup_timeout)
        }),
    )
    .await
    {
        Ok(Ok(Some(removed))) => removed,
        Ok(Ok(None)) | Ok(Err(_)) | Err(_) => {
            let envelope = json!({
                "removed": 0,
                "older_than_hours": older_than_hours,
                "error": "retention_prune_failed",
            });
            let text = serde_json::to_string(&envelope).unwrap_or_default();
            tracing::warn!("jobs cleanup exceeded bounded timeout or worker failed");
            return CallToolResult {
                content: vec![ToolContent::Text { text }],
                structured_content: Some(envelope),
                is_error: true,
                meta: None,
            };
        }
    };
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

    use dcc_mcp_actions::{ToolDispatcher, ToolRegistry, registry::ToolMeta};
    use dcc_mcp_job::job::{JobManager, JobProgress};
    use dcc_mcp_job::poller::{JobPollRegistration, PollContract};
    use dcc_mcp_models::{NextTools, SkillToolAnnotations};
    use dcc_mcp_skills::SkillCatalog;
    use tempfile::tempdir;

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
    fn jobs_get_status_projects_split_phase_error_as_structured_envelope() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(ToolMeta {
            name: "split_tool".into(),
            ..Default::default()
        });
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let jobs = Arc::new(JobManager::new());
        let handle = jobs.create("split_tool");
        let id = handle.read().id.clone();
        jobs.start(&id).unwrap();
        jobs.fail(
            &id,
            crate::split_phase::project_error_for_job(
                "SPLIT_PHASE_TIMEOUT: continuation timed out",
            ),
        )
        .unwrap();
        let state = ServerState::builder(registry, dispatcher, catalog)
            .with_jobs(jobs)
            .build();
        let payload = handle_jobs_get_status(&state, &json!({"job_id": id}));
        let error = payload.structured_content.unwrap()["error"].clone();
        assert_eq!(error["layer"], "instance");
        assert_eq!(error["code"], "SPLIT_PHASE_TIMEOUT");
    }

    #[test]
    fn jobs_get_status_preserves_json_looking_legacy_error_strings() {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(ToolMeta {
            name: "legacy_tool".into(),
            ..Default::default()
        });
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let jobs = Arc::new(JobManager::new());
        let handle = jobs.create("legacy_tool");
        let id = handle.read().id.clone();
        jobs.start(&id).unwrap();
        jobs.fail(&id, r#"{"reason":"bad"}"#).unwrap();
        let state = ServerState::builder(registry, dispatcher, catalog)
            .with_jobs(jobs)
            .build();
        let payload = handle_jobs_get_status(&state, &json!({"job_id": id}));
        assert_eq!(
            payload.structured_content.unwrap()["error"],
            r#"{"reason":"bad"}"#
        );
    }

    fn state() -> ServerState {
        let registry = Arc::new(ToolRegistry::new());
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        ServerState::builder(registry, dispatcher, catalog).build()
    }

    #[test]
    fn poll_contract_query_names_unregistered_job_types() {
        let state = state();

        let payload = handle_jobs_poll_contract(
            &state,
            &json!({"job_type": "blender_render__render_sequence"}),
        );
        let envelope = payload.structured_content.unwrap();

        assert_eq!(envelope["registered"], false);
        assert_eq!(envelope["reason"], "job_type_not_registered");
        assert_eq!(
            envelope["job_type"], "blender_render__render_sequence",
            "an untracked job type is diagnosable instead of failing silently"
        );

        state
            .poll_registry
            .register(JobPollRegistration::new(
                "blender_render__render_sequence",
                PollContract::adapter("blender_render__get_render_job"),
            ))
            .expect("register");

        let payload = handle_jobs_poll_contract(
            &state,
            &json!({"job_type": "blender_render__render_sequence"}),
        );
        let envelope = payload.structured_content.unwrap();
        assert_eq!(envelope["registered"], true);
        assert_eq!(envelope["poll"]["tool"], "blender_render__get_render_job");

        let listing = handle_jobs_poll_contract(&state, &json!({}))
            .structured_content
            .unwrap();
        assert_eq!(listing["count"], 1);
        assert_eq!(
            listing["registered_types"][0]["job_type"],
            "blender_render__render_sequence"
        );
    }

    #[test]
    fn status_query_prefers_frames_on_disk_over_a_cached_zero_counter() {
        let dir = tempdir().expect("tempdir");
        for frame in ["frame.0001.exr", "frame.0002.exr", "frame.0003.exr"] {
            std::fs::write(dir.path().join(frame), b"x").expect("write frame");
        }
        std::fs::write(dir.path().join("log.txt"), b"x").expect("write log");

        let state = state();
        let handle = state.jobs.create("render__render_rop");
        let id = handle.read().id.clone();
        state.jobs.start(&id).expect("start");
        state
            .jobs
            .update_progress(
                &id,
                dcc_mcp_job::job::JobProgress {
                    current: 0,
                    total: 24,
                    message: None,
                },
            )
            .expect("progress");
        state
            .jobs
            .complete(
                &id,
                json!({
                    "success": true,
                    "output_dir": dir.path().to_string_lossy(),
                    "output_extensions": ["exr"],
                }),
            )
            .expect("complete");

        let payload = handle_jobs_get_status(&state, &json!({"job_id": id}));
        let envelope = payload.structured_content.unwrap();

        assert_eq!(envelope["status"], "completed");
        assert_eq!(
            envelope["progress"]["current"], 3,
            "the on-disk count wins over a cached zero"
        );
        assert_eq!(envelope["progress"]["total"], 24);
        assert_eq!(envelope["progress"]["counter_source"], "disk");
        assert_eq!(envelope["progress"]["reported_current"], 0);
    }

    #[test]
    fn running_job_reconciles_against_the_output_dir_it_declared_at_launch() {
        let dir = tempdir().expect("tempdir");
        for frame in ["frame.0001.exr", "frame.0002.exr"] {
            std::fs::write(dir.path().join(frame), b"x").expect("write frame");
        }
        std::fs::write(dir.path().join("log.txt"), b"x").expect("write log");

        let state = state();
        let handle = state.jobs.create("render__render_rop");
        let id = handle.read().id.clone();
        // The launch arguments are the only place a Running job can declare
        // where its outputs land; `complete()` has not happened yet.
        state.jobs.set_output(
            &id,
            Some(dcc_mcp_job::job::JobOutputTarget {
                dir: dir.path().to_string_lossy().into_owned(),
                extensions: vec!["exr".to_string()],
            }),
        );
        state.jobs.start(&id).expect("start");
        state
            .jobs
            .update_progress(
                &id,
                dcc_mcp_job::job::JobProgress {
                    current: 0,
                    total: 24,
                    message: None,
                },
            )
            .expect("progress");

        let payload = handle_jobs_get_status(&state, &json!({"job_id": id}));
        let envelope = payload.structured_content.unwrap();

        assert_eq!(envelope["status"], "running");
        assert_eq!(
            envelope["progress"]["current"], 2,
            "a running job counts the frames already on disk instead of the cached zero"
        );
        assert_eq!(envelope["progress"]["total"], 24);
        assert_eq!(envelope["progress"]["counter_source"], "disk");
        assert_eq!(envelope["progress"]["reported_current"], 0);
    }

    #[test]
    fn running_job_without_a_declared_output_dir_keeps_its_reported_counter() {
        let state = state();
        let handle = state.jobs.create("render__render_rop");
        let id = handle.read().id.clone();
        state.jobs.start(&id).expect("start");
        state
            .jobs
            .update_progress(
                &id,
                dcc_mcp_job::job::JobProgress {
                    current: 0,
                    total: 24,
                    message: None,
                },
            )
            .expect("progress");

        let payload = handle_jobs_get_status(&state, &json!({"job_id": id}));
        let envelope = payload.structured_content.unwrap();

        assert_eq!(envelope["status"], "running");
        assert_eq!(envelope["progress"]["current"], 0);
        assert_eq!(envelope["progress"]["counter_source"], "reported");
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
            annotations: SkillToolAnnotations {
                read_only_hint: Some(true),
                idempotent_hint: Some(true),
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
            payload["adapter_job"]["poll_contract"],
            json!({"registered": true, "reason": null})
        );
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
