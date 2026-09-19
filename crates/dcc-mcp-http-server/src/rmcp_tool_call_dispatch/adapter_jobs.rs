//! Adapter-owned job discovery and poll registration.

use serde_json::{Value, json};

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_job::poller::{
    JobPollRegistration, PollContract, is_known_job_status, is_terminal_job_status,
    output_counter_from_result, reconcile_progress_value,
};
use dcc_mcp_models::ExecutionMode;

use crate::server_state::ServerState;

pub(crate) fn attach_direct_adapter_job_contract(
    state: &ServerState,
    action: &ToolMeta,
    output: &mut Value,
) {
    let Some(job) = direct_adapter_job(output) else {
        return;
    };
    // An adapter that already declares how to poll its own job wins over the
    // `next_tools` heuristic (issue #2262). Without this the descriptor is
    // stripped below and the job type stays untracked by the poller, which is
    // exactly how `--wait` became impossible for one render job type.
    let declared = PollContract::from_value(
        output
            .get("adapter_job")
            .and_then(|value| value.get("poll")),
    )
    .or_else(|| PollContract::from_value(output.get("poll")));
    // Whatever wins is registered, so a job type the envelope claims is
    // waitable is a job type `jobs_poll_contract` also knows about.
    let contract = ensure_poll_contract(state, action, output, declared);
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
        Value::Bool(is_terminal_job_status(&job.status)),
    );
    descriptor_object.insert(
        "poll_contract".to_string(),
        json!({
            "registered": contract.is_some(),
            "reason": contract.is_none().then_some("safe_poll_tool_not_declared"),
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
    if let Some(progress) = reconcile_reported_progress(output, job.progress.as_ref()) {
        descriptor_object.insert("progress".to_string(), progress);
    } else if let Some(progress) = job.progress {
        descriptor_object.insert("progress".to_string(), progress);
    }
    if let Some(contract) = contract.as_ref() {
        // Replayed verbatim, argument name included — including a declared
        // `argument_field` that is not `job_id`.
        descriptor_object.insert("poll".to_string(), contract.to_value(&job.job_id));
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
    // The unified poller wins: an explicit registration is how a job type
    // becomes waitable without declaring a discoverable follow-up tool.
    if let Some(tool) = state.poll_registry.poll_tool(&action.name) {
        return Some(tool);
    }
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

/// Resolve — and remember — how `action`'s job type is polled (issue #2262).
///
/// Precedence: a contract the adapter declared on its own result, then one the
/// poller already knows, then the legacy `next_tools` heuristic. Whichever
/// wins is registered, so the envelope's `poll_contract.registered` and
/// `jobs_poll_contract` can never disagree: reporting a poll tool the registry
/// does not know would let `--wait` claim a job type is tracked when the
/// poller has no contract for it.
pub(crate) fn ensure_poll_contract(
    state: &ServerState,
    action: &ToolMeta,
    output: &Value,
    declared: Option<PollContract>,
) -> Option<PollContract> {
    let contract = declared
        .or_else(|| {
            state
                .poll_registry
                .resolve(&action.name)
                .map(|registration| registration.poll)
        })
        .or_else(|| adapter_poll_tool(state, action).map(PollContract::adapter))?;
    register_poll_contract(state, &action.name, &contract, output).then_some(contract)
}

/// Remember how `job_type` is polled so later waits and `jobs_poll_contract`
/// queries see the same contract (issue #2262).
///
/// Returns `false` when the registry rejected the registration, so callers do
/// not advertise a contract that cannot be resolved again.
fn register_poll_contract(
    state: &ServerState,
    job_type: &str,
    contract: &PollContract,
    output: &Value,
) -> bool {
    let mut registration = JobPollRegistration::new(job_type, contract.clone());
    if let Some(counter) = output_counter_from_result(output) {
        registration = registration.with_output(counter);
    }
    if let Err(error) = state.poll_registry.register(registration) {
        tracing::debug!(
            job_type = %job_type,
            error = %error,
            "ignoring invalid job poll registration"
        );
        return false;
    }
    true
}

/// Recompute a reported counter from the job's output directory on disk.
///
/// Renderers that cache their own counter can report `0` while frames are
/// already written (#2262); the on-disk count is authoritative.
fn reconcile_reported_progress(output: &Value, reported: Option<&Value>) -> Option<Value> {
    let counter = output_counter_from_result(output)?;
    reconcile_progress_value(reported, counter.count())
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
        if job_id.is_empty() || !is_known_job_status(status) {
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
    use std::sync::Arc;

    use super::*;
    use dcc_mcp_actions::{ToolDispatcher, ToolRegistry};
    use dcc_mcp_models::NextTools;
    use dcc_mcp_models::SkillToolAnnotations;
    use dcc_mcp_skills::SkillCatalog;
    use tempfile::tempdir;

    fn state_with(poll_meta: ToolMeta) -> ServerState {
        let registry = Arc::new(ToolRegistry::new());
        registry.register_action(poll_meta);
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        ServerState::builder(registry, dispatcher, catalog).build()
    }

    #[test]
    fn declared_poll_descriptor_registers_the_job_type_with_the_poller() {
        // The adapter declares the contract itself and declares no
        // `next_tools` follow-up: before #2262 this job type was untracked, so
        // `--wait` could not reach a terminal state for it.
        let state = state_with(ToolMeta {
            name: "blender_render__render_sequence".to_string(),
            ..Default::default()
        });
        let action = state
            .registry
            .get_action("blender_render__render_sequence", None)
            .expect("launch action");
        let mut output = json!({
            "success": true,
            "job_id": "render-42",
            "status": "running",
            "progress": {"current": 0, "total": 24},
            "poll": {
                "owner": "adapter",
                "tool": "blender_render__get_render_job",
                "arguments": {"job_id": "render-42"},
            },
        });

        attach_direct_adapter_job_contract(&state, &action, &mut output);

        assert_eq!(output["adapter_job"]["poll_contract"]["registered"], true);
        assert_eq!(
            output["adapter_job"]["poll"],
            json!({
                "owner": "adapter",
                "tool": "blender_render__get_render_job",
                "arguments": {"job_id": "render-42"},
            }),
            "the declared contract survives instead of being stripped"
        );
        assert_eq!(
            state
                .poll_registry
                .poll_tool("blender_render__render_sequence")
                .as_deref(),
            Some("blender_render__get_render_job")
        );
    }

    #[test]
    fn declared_poll_descriptor_with_extra_arguments_is_rejected() {
        // A contract carries exactly one argument field, so replaying
        // `{"job_id": ..., "scene_id": ...}` would drop `scene_id` and the
        // `--wait` poll call would reach the tool with a missing required
        // input. Rejecting the descriptor is the honest outcome: the job type
        // stays untracked and the wait reports why.
        let state = state_with(ToolMeta {
            name: "houdini_render__render_rop".to_string(),
            ..Default::default()
        });
        let action = state
            .registry
            .get_action("houdini_render__render_rop", None)
            .expect("launch action");
        let mut output = json!({
            "job_id": "rop-7",
            "status": "running",
            "poll": {
                "owner": "adapter",
                "tool": "houdini_render__get_render_job",
                "arguments": {"job_id": "rop-7", "scene_id": "scene-2"},
            },
        });

        attach_direct_adapter_job_contract(&state, &action, &mut output);

        assert_eq!(output["adapter_job"]["poll_contract"]["registered"], false);
        assert_eq!(
            output["adapter_job"]["poll_contract"]["reason"],
            "safe_poll_tool_not_declared"
        );
        assert!(
            output["adapter_job"].get("poll").is_none(),
            "no lossy single-argument replay may reach the --wait poll call, \
             got {:?}",
            output["adapter_job"].get("poll")
        );
        assert_eq!(
            state.poll_registry.poll_tool("houdini_render__render_rop"),
            None,
            "an unusable descriptor is rejected instead of registered"
        );
    }

    #[test]
    fn declared_poll_descriptor_replays_every_argument_it_declared() {
        // The counterpart to the rejection above: a single-argument descriptor
        // keeps its own argument name, so nothing is lost on replay.
        let state = state_with(ToolMeta {
            name: "houdini_render__render_rop".to_string(),
            ..Default::default()
        });
        let action = state
            .registry
            .get_action("houdini_render__render_rop", None)
            .expect("launch action");
        let mut output = json!({
            "job_id": "rop-7",
            "status": "running",
            "poll": {
                "owner": "adapter",
                "tool": "houdini_render__get_render_job",
                "arguments": {"render_id": "rop-7"},
            },
        });

        attach_direct_adapter_job_contract(&state, &action, &mut output);

        assert_eq!(
            output["adapter_job"]["poll"],
            json!({
                "owner": "adapter",
                "tool": "houdini_render__get_render_job",
                "arguments": {"render_id": "rop-7"},
            })
        );
    }

    #[test]
    fn heuristic_poll_fallback_is_registered_with_the_poller() {
        // `poll_contract.registered` and `jobs_poll_contract` must agree: the
        // legacy `next_tools` heuristic used to emit a poll descriptor without
        // registering the job type, so the envelope claimed a tracked job type
        // the poller had never heard of.
        let state = state_with(poll_meta());
        state.registry.register_action(ToolMeta {
            name: "render__render_rop".to_string(),
            next_tools: NextTools {
                on_success: vec!["render__get_job".to_string()],
                ..Default::default()
            },
            ..Default::default()
        });
        let action = state
            .registry
            .get_action("render__render_rop", None)
            .expect("launch action");
        let mut output = json!({"job_id": "rop-9", "status": "running"});

        attach_direct_adapter_job_contract(&state, &action, &mut output);

        assert_eq!(output["adapter_job"]["poll_contract"]["registered"], true);
        assert_eq!(output["adapter_job"]["poll"]["tool"], "render__get_job");
        assert_eq!(
            state
                .poll_registry
                .poll_tool("render__render_rop")
                .as_deref(),
            Some("render__get_job"),
            "a heuristic fallback becomes a real registration"
        );
        assert_eq!(
            state
                .poll_registry
                .resolve("render__render_rop")
                .map(|registration| registration.poll.argument_field),
            Some("job_id".to_string())
        );
    }

    #[test]
    fn declared_output_dir_makes_the_counter_disk_backed() {
        let dir = tempdir().expect("tempdir");
        for frame in ["frame.0001.exr", "frame.0002.exr"] {
            std::fs::write(dir.path().join(frame), b"x").expect("write frame");
        }
        let state = state_with(ToolMeta {
            name: "blender_render__render_sequence".to_string(),
            ..Default::default()
        });
        let action = state
            .registry
            .get_action("blender_render__render_sequence", None)
            .expect("launch action");
        let mut output = json!({
            "job_id": "render-42",
            "status": "running",
            "output_dir": dir.path().to_string_lossy(),
            "output_extensions": ["exr"],
            "progress": {"current": 0, "total": 24},
        });

        attach_direct_adapter_job_contract(&state, &action, &mut output);

        assert_eq!(output["adapter_job"]["progress"]["current"], 2);
        assert_eq!(output["adapter_job"]["progress"]["total"], 24);
        assert_eq!(output["adapter_job"]["progress"]["counter_source"], "disk");
    }

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
