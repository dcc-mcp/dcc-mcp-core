//! `tools/call` routing for rmcp handlers.

mod handlers;
mod helpers;
mod thread_route;
mod wire;

pub(crate) use thread_route::dispatch_action_with_thread_routing_cancellable;
pub use thread_route::{ThreadRoutingDispatch, dispatch_action_with_thread_routing};
pub(crate) use wire::{decode_dispatch_output, encode_dispatch_wire, use_main_thread_route};

use serde_json::Value;

use dcc_mcp_jsonrpc::{CallToolMeta, CallToolResult, coerce_tool_arguments_object};
use dcc_mcp_protocols::error_envelope::DccMcpError;

use crate::dynamic_tools::DYNAMIC_TOOL_PREFIX;
use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_async::{async_dispatch_config, dispatch_async_registry_tool};
use crate::server_state::ServerState;

use handlers::{
    handle_activate_tool_group, handle_deactivate_tool_group, handle_deregister_tool_dynamic,
    handle_describe_action, handle_get_skill_info, handle_jobs_cleanup, handle_jobs_get_status,
    handle_list_actions, handle_list_dynamic_tools_dynamic, handle_list_roots, handle_list_skills,
    handle_load_skill, handle_register_tool_dynamic, handle_search_skills, handle_search_tools,
    handle_unload_skill, route_dynamic_execution,
};
use helpers::{
    attach_next_tools_meta, capability_gate_result, dispatch_err_result, dispatch_json_result,
    handle_stub_tool, readiness_gate_result, resolve_action_name,
};
use thread_route::execute_threaded_dispatch;

/// Decode rmcp request `_meta` into our JSON-RPC [`CallToolMeta`] shape.
pub(crate) fn call_meta_from_rmcp(meta: Option<&rmcp::model::Meta>) -> Option<CallToolMeta> {
    meta.and_then(|m| serde_json::from_value(Value::Object(m.0.clone())).ok())
}

/// Central entry — mirrors JSON-RPC [`resolve_tool_call`] + registry dispatch (#727736b-era).
pub async fn dispatch_rmcp_tool_call(
    state: &ServerState,
    registry_ctx: &RegistryContext,
    session_id: Option<&str>,
    tool_name: &str,
    arguments: Option<Value>,
    call_meta: Option<&CallToolMeta>,
) -> Result<CallToolResult, String> {
    let arguments_value = coerce_tool_arguments_object(arguments)?;

    if tool_name == "call_action" && state.lazy_actions {
        return handle_call_action_async(
            state,
            registry_ctx,
            session_id,
            call_meta,
            arguments_value,
        )
        .await;
    }

    match tool_name {
        "list_roots" => Ok(handle_list_roots(state, session_id)),
        "list_skills" => Ok(handle_list_skills(state, &arguments_value)),
        "get_skill_info" => Ok(handle_get_skill_info(state, &arguments_value)),
        "load_skill" => Ok(handle_load_skill(
            state,
            registry_ctx,
            &arguments_value,
            session_id,
        )),
        "unload_skill" => Ok(handle_unload_skill(
            state,
            registry_ctx,
            &arguments_value,
            session_id,
        )),
        "search_skills" => Ok(handle_search_skills(state, &arguments_value)),
        "activate_tool_group" => Ok(handle_activate_tool_group(
            state,
            &arguments_value,
            session_id,
        )),
        "deactivate_tool_group" => Ok(handle_deactivate_tool_group(
            state,
            &arguments_value,
            session_id,
        )),
        "search_tools" => Ok(handle_search_tools(state, &arguments_value)),
        "jobs_get_status" => Ok(handle_jobs_get_status(state, &arguments_value)),
        "jobs_cleanup" => Ok(handle_jobs_cleanup(state, &arguments_value)),
        "register_tool" => Ok(handle_register_tool_dynamic(
            state,
            session_id,
            &arguments_value,
        )),
        "deregister_tool" => Ok(handle_deregister_tool_dynamic(
            state,
            session_id,
            &arguments_value,
        )),
        "list_dynamic_tools" => Ok(handle_list_dynamic_tools_dynamic(state, session_id)),
        "list_actions" if state.lazy_actions => Ok(handle_list_actions(state, &arguments_value)),
        "describe_action" if state.lazy_actions => {
            Ok(handle_describe_action(state, &arguments_value, session_id))
        }
        name => {
            dispatch_non_core_tool(
                state,
                registry_ctx,
                session_id,
                call_meta,
                name,
                arguments_value,
            )
            .await
        }
    }
}

async fn dispatch_non_core_tool(
    state: &ServerState,
    registry_ctx: &RegistryContext,
    session_id: Option<&str>,
    call_meta: Option<&CallToolMeta>,
    tool_name: &str,
    arguments_value: Value,
) -> Result<CallToolResult, String> {
    if let Some(r) = handle_stub_tool(tool_name) {
        return Ok(r);
    }
    if tool_name.starts_with(DYNAMIC_TOOL_PREFIX)
        && let Some(r) =
            route_dynamic_execution(state, session_id, tool_name, arguments_value.clone())
    {
        return Ok(r);
    }
    dispatch_registry_tool(
        state,
        registry_ctx,
        session_id,
        call_meta,
        tool_name,
        arguments_value,
    )
    .await
}

async fn handle_call_action_async(
    state: &ServerState,
    registry_ctx: &RegistryContext,
    session_id: Option<&str>,
    call_meta: Option<&CallToolMeta>,
    arguments_value: Value,
) -> Result<CallToolResult, String> {
    let args = &arguments_value;
    let id = match args.get("id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return Ok(CallToolResult::error("Missing required parameter: id")),
    };

    if matches!(
        id.as_str(),
        "list_actions" | "describe_action" | "call_action"
    ) {
        let envelope = DccMcpError::new(
            "registry",
            "RECURSIVE_META_CALL",
            format!("`call_action` refuses to dispatch meta-tool `{id}`."),
        )
        .with_hint("Call the meta-tool directly via tools/call instead.");
        return Ok(CallToolResult::error(envelope.to_json().to_string()));
    }

    let inner_args = args.get("args").cloned();

    Box::pin(dispatch_rmcp_tool_call(
        state,
        registry_ctx,
        session_id,
        &id,
        inner_args,
        call_meta,
    ))
    .await
}

async fn dispatch_registry_tool(
    state: &ServerState,
    registry_ctx: &RegistryContext,
    session_id: Option<&str>,
    call_meta: Option<&CallToolMeta>,
    tool_name: &str,
    call_params: Value,
) -> Result<CallToolResult, String> {
    let resolved_name = resolve_action_name(state, tool_name);
    let action_meta = match state.registry.get_action(&resolved_name, None) {
        Some(meta) => meta,
        None => {
            let envelope = DccMcpError::new(
                "registry",
                "ACTION_NOT_FOUND",
                format!("Unknown tool: {tool_name}"),
            )
            .with_hint(
                "Use tools/list to see available tools, or load a skill first with load_skill."
                    .to_string(),
            );
            return Ok(CallToolResult::error(envelope.to_json().to_string()));
        }
    };

    if let Some(r) = capability_gate_result(state, &resolved_name, &action_meta) {
        return Ok(r);
    }
    if let Some(r) = readiness_gate_result(state, registry_ctx, tool_name) {
        return Ok(r);
    }

    if let Some(cfg) = async_dispatch_config(call_meta, &action_meta) {
        return Ok(dispatch_async_registry_tool(
            state,
            session_id,
            resolved_name,
            call_params,
            call_meta.and_then(|meta| serde_json::to_value(meta).ok()),
            cfg,
        )
        .await);
    }

    let dispatch_out = execute_threaded_dispatch(
        state,
        &resolved_name,
        call_params.clone(),
        None,
        action_meta.thread_affinity,
        action_meta.enforce_thread_affinity,
    )
    .await;

    let mut result = match dispatch_out {
        Ok(output) => dispatch_json_result(output),
        Err(e) => dispatch_err_result(&resolved_name, e),
    };

    attach_next_tools_meta(&mut result, &action_meta.next_tools);
    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;

    use std::sync::Arc;
    use std::time::Duration;

    use dcc_mcp_actions::ToolDispatcher;
    use dcc_mcp_actions::registry::{ToolMeta, ToolRegistry};
    use dcc_mcp_job::job::JobStatus;
    use dcc_mcp_jsonrpc::ToolContent;
    use dcc_mcp_models::{ExecutionMode, SkillScope, ThreadAffinity};
    use dcc_mcp_skill_rest::StaticReadiness;
    use dcc_mcp_skills::SkillCatalog;
    use serde_json::json;

    use crate::executor::InProcessExecutor;
    use crate::mcp_tool_list_builder::assemble_full_tool_list;

    fn skill_tool_meta(name: &str, skill_name: &str) -> ToolMeta {
        ToolMeta {
            name: name.to_string(),
            description: format!("{skill_name} create cube"),
            dcc: "maya".to_string(),
            input_schema: json!({"type": "object"}),
            skill_name: Some(skill_name.to_string()),
            ..Default::default()
        }
    }

    fn ready_context() -> RegistryContext {
        RegistryContext {
            resource_provider: None,
            prompt_provider: None,
            readiness: Arc::new(StaticReadiness::fully_ready()),
            on_skill_catalog_mutated: Arc::new(|| {}),
        }
    }

    fn result_text_json(result: &dcc_mcp_jsonrpc::CallToolResult) -> Value {
        serde_json::from_str(result_text(result)).expect("handler text should be JSON")
    }

    fn result_text(result: &dcc_mcp_jsonrpc::CallToolResult) -> &str {
        let Some(ToolContent::Text { text }) = result.content.first() else {
            panic!("expected text content, got {result:?}");
        };
        text
    }

    #[tokio::test]
    async fn skipped_skill_diagnostics_are_visible_through_mcp_discovery() {
        let temp = tempfile::tempdir().unwrap();
        let skill_dir = temp.path().join("maya-mgear");
        std::fs::create_dir_all(&skill_dir).unwrap();
        std::fs::write(
            skill_dir.join(dcc_mcp_skills::constants::SKILL_METADATA_FILE),
            "---\nname: maya-mgear\ndescription: mGear integration\nversion: \"1.0.0\"\n---\n# body\n",
        )
        .unwrap();

        let registry = Arc::new(ToolRegistry::new());
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let roots = vec![temp.path().to_string_lossy().to_string()];
        catalog.discover(Some(&roots), Some("maya"));
        let state = ServerState::builder(registry, dispatcher, catalog).build();

        let search = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_skills",
            Some(json!({"query": "mgear"})),
            None,
        )
        .await
        .expect("search_skills dispatch should succeed");
        assert!(!search.is_error);
        let search_payload = result_text_json(&search);
        assert_eq!(search_payload["skipped_count"], 1);
        assert_eq!(search_payload["skipped"][0]["skill_name"], "maya-mgear");
        assert!(
            search_payload["skipped"][0]["suggested_fix"]
                .as_str()
                .unwrap()
                .contains("metadata.dcc-mcp.version")
        );

        let listed = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "list_skills",
            Some(json!({"status": "skipped"})),
            None,
        )
        .await
        .expect("list_skills dispatch should succeed");
        let list_payload = result_text_json(&listed);
        assert_eq!(list_payload["skipped_count"], 1);
        assert_eq!(
            list_payload["skipped"][0]["reason_code"],
            "non_spec_top_level_keys"
        );

        let info = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "get_skill_info",
            Some(json!({"skill_name": "maya-mgear"})),
            None,
        )
        .await
        .expect("get_skill_info dispatch should succeed");
        assert!(info.is_error);
        let info_payload = result_text_json(&info);
        assert_eq!(info_payload["error"], "skill_skipped");
        assert!(
            !info_payload
                .to_string()
                .contains(temp.path().to_string_lossy().as_ref())
        );
    }

    #[tokio::test]
    async fn skipped_skill_diagnostics_obey_search_filters_and_pagination() {
        let temp = tempfile::tempdir().unwrap();
        let maya_dir = temp.path().join("maya-broken");
        let blender_dir = temp.path().join("blender-broken");
        std::fs::create_dir_all(&maya_dir).unwrap();
        std::fs::create_dir_all(&blender_dir).unwrap();
        std::fs::write(
            maya_dir.join(dcc_mcp_skills::constants::SKILL_METADATA_FILE),
            "---\nname: maya-broken\ndescription: Broken Maya skill\nversion: \"1.0.0\"\nmetadata:\n  dcc-mcp:\n    dcc: maya\n    tags: [rigging]\n---\n# body\n",
        )
        .unwrap();
        std::fs::write(
            blender_dir.join(dcc_mcp_skills::constants::SKILL_METADATA_FILE),
            "---\nname: blender-broken\ndescription: Broken Blender skill\nversion: \"1.0.0\"\nmetadata:\n  dcc-mcp:\n    dcc: blender\n    tags: [modeling]\n---\n# body\n",
        )
        .unwrap();

        let registry = Arc::new(ToolRegistry::new());
        let dispatcher = Arc::new(ToolDispatcher::new((*registry).clone()));
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let roots = vec![temp.path().to_string_lossy().to_string()];
        catalog.discover_scoped(&[(SkillScope::System, roots)], None);
        let state = ServerState::builder(registry, dispatcher, catalog).build();

        let filtered = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_skills",
            Some(json!({
                "query": "broken",
                "dcc": "blender",
                "scope": "system",
                "limit": 1
            })),
            None,
        )
        .await
        .expect("search_skills dispatch should succeed");
        let filtered_payload = result_text_json(&filtered);
        assert_eq!(filtered_payload["total"], 1);
        assert_eq!(filtered_payload["skill_total"], 0);
        assert_eq!(filtered_payload["skipped_count"], 1);
        assert_eq!(
            filtered_payload["skipped"][0]["skill_name"],
            "blender-broken"
        );
        assert_eq!(filtered_payload["skipped"][0]["dcc"], "blender");
        assert_eq!(filtered_payload["skipped"][0]["scope"], "system");

        let tag_filtered = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_skills",
            Some(json!({"query": "broken", "tags": ["rigging"], "limit": 20})),
            None,
        )
        .await
        .expect("search_skills dispatch should succeed");
        let tag_payload = result_text_json(&tag_filtered);
        assert_eq!(tag_payload["skipped_count"], 1);
        assert_eq!(tag_payload["skipped"][0]["skill_name"], "maya-broken");

        let wrong_scope = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "search_skills",
            Some(json!({"query": "broken", "scope": "repo"})),
            None,
        )
        .await
        .expect("search_skills dispatch should succeed");
        assert_eq!(
            result_text(&wrong_scope),
            "No skills found matching 'broken'."
        );

        let listed = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "list_skills",
            Some(json!({"status": "skipped", "limit": 1, "offset": 1})),
            None,
        )
        .await
        .expect("list_skills dispatch should succeed");
        let list_payload = result_text_json(&listed);
        assert_eq!(list_payload["total"], 2);
        assert_eq!(list_payload["skipped_count"], 2);
        assert_eq!(list_payload["limit"], 1);
        assert_eq!(list_payload["offset"], 1);
        assert_eq!(list_payload["truncated"], false);
        assert_eq!(list_payload["skipped"].as_array().unwrap().len(), 1);
        assert_eq!(list_payload["skills"].as_array().unwrap().len(), 0);
    }

    #[tokio::test]
    async fn skill_qualified_collision_name_from_tools_list_dispatches() {
        let registry = ToolRegistry::new();
        let dispatcher = Arc::new(ToolDispatcher::new(registry.clone()));

        registry.register_action(skill_tool_meta(
            "maya_modeling__create_cube",
            "maya-modeling",
        ));
        registry.register_action(skill_tool_meta("maya_rigging__create_cube", "maya-rigging"));
        dispatcher.register_handler("maya_modeling__create_cube", |_| {
            Ok(json!({"skill": "modeling"}))
        });
        dispatcher.register_handler("maya_rigging__create_cube", |_| {
            Ok(json!({"skill": "rigging"}))
        });

        let registry = Arc::new(registry);
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let state = ServerState::builder(registry, dispatcher, catalog).build();

        let listed_names: Vec<String> = assemble_full_tool_list(&state, false, None)
            .into_iter()
            .map(|tool| tool.name)
            .collect();
        assert!(
            listed_names
                .iter()
                .any(|name| name == "maya-modeling__create_cube")
        );
        assert!(
            listed_names
                .iter()
                .any(|name| name == "maya-rigging__create_cube")
        );
        assert!(!listed_names.iter().any(|name| name == "create_cube"));

        let result = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "maya-modeling__create_cube",
            Some(json!({})),
            None,
        )
        .await
        .expect("dispatch should not return transport error");

        assert!(
            !result.is_error,
            "tools/list name must be callable: {result:?}"
        );
        assert_eq!(
            result.structured_content,
            Some(json!({"skill": "modeling"}))
        );
    }

    #[tokio::test]
    async fn rich_image_skill_result_adds_native_mcp_image_content() {
        let registry = ToolRegistry::new();
        let dispatcher = Arc::new(ToolDispatcher::new(registry.clone()));
        let output = json!({
            "success": true,
            "message": "Viewport captured",
            "context": {
                "__rich__": {
                    "kind": "image",
                    "data": "iVBORw0KGgo=",
                    "mime": "image/png"
                }
            }
        });

        registry.register_action(skill_tool_meta(
            "dcc_diagnostics__screenshot",
            "dcc-diagnostics",
        ));
        dispatcher.register_handler("dcc_diagnostics__screenshot", {
            let output = output.clone();
            move |_| Ok(output.clone())
        });

        let registry = Arc::new(registry);
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let state = ServerState::builder(registry, dispatcher, catalog).build();

        let result = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "dcc-diagnostics__screenshot",
            Some(json!({})),
            None,
        )
        .await
        .expect("rich image skill dispatch should succeed");

        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/context/__rich__/data")),
            Some(&json!("<omitted; see native MCP image content>"))
        );
        assert_eq!(result.content.len(), 2);
        let text_output = result_text_json(&result);
        assert_eq!(
            text_output.pointer("/context/__rich__/data"),
            Some(&json!("<omitted; see native MCP image content>"))
        );
        assert!(matches!(
            &result.content[1],
            ToolContent::Image { data, mime_type }
                if data == "iVBORw0KGgo=" && mime_type == "image/png"
        ));

        let rmcp_result = crate::rmcp_adapter::call_result_to_rmcp(&result);
        assert!(matches!(
            &rmcp_result.content[1].raw,
            rmcp::model::RawContent::Image(image)
                if image.data == "iVBORw0KGgo=" && image.mime_type == "image/png"
        ));
    }

    #[test]
    fn malformed_rich_image_is_redacted_even_with_preexisting_artifact_path() {
        let encoded = "%%%private-invalid-base64%%%";
        let result = dispatch_json_result(json!({
            "success": true,
            "context": {
                "__rich__": {
                    "kind": "image",
                    "data": encoded,
                    "mime": "image/png",
                    "artifact_path": "C:/existing/capture.png"
                }
            }
        }));

        assert_eq!(
            result.content.len(),
            1,
            "invalid data must not become MCP ImageContent"
        );
        let safe = result.structured_content.as_ref().unwrap();
        assert_eq!(
            safe.pointer("/context/__rich__/data"),
            Some(&json!("<omitted; invalid inline image data>"))
        );
        assert_eq!(
            safe.pointer("/context/__rich__/native_image_error"),
            Some(&json!("invalid base64 image data"))
        );
        assert_eq!(
            safe.pointer("/context/__rich__/artifact_path"),
            Some(&json!("C:/existing/capture.png"))
        );
        assert!(!serde_json::to_string(&result).unwrap().contains(encoded));
    }

    #[test]
    fn computer_use_failure_is_mcp_error_with_sanitized_native_image() {
        let encoded = "iVBORw0KGgo=";
        let result = dispatch_json_result(json!({
            "success": false,
            "message": "Computer Use was interrupted",
            "error": "user_interrupted",
            "context": {
                "__rich__": {
                    "kind": "image",
                    "data": encoded,
                    "mime": "image/png"
                }
            }
        }));

        assert!(
            result.is_error,
            "success=false must use MCP error semantics"
        );
        assert_eq!(
            result
                .structured_content
                .as_ref()
                .and_then(|value| value.pointer("/context/__rich__/data")),
            Some(&json!("<omitted; see native MCP image content>"))
        );
        assert!(!result_text(&result).contains(encoded));
        assert!(matches!(
            &result.content[1],
            ToolContent::Image { data, mime_type }
                if data == encoded && mime_type == "image/png"
        ));
    }

    #[tokio::test]
    async fn async_main_thread_job_decodes_deferred_dispatch_wire() {
        let registry = ToolRegistry::new();
        let dispatcher = Arc::new(ToolDispatcher::new(registry.clone()));
        let seen_meta = Arc::new(parking_lot::Mutex::new(None::<Value>));

        registry.register_action(ToolMeta {
            name: "main_thread_job".to_string(),
            description: "main-thread async job".to_string(),
            dcc: "maya".to_string(),
            input_schema: json!({"type": "object"}),
            execution: ExecutionMode::Sync,
            timeout_hint_secs: Some(5),
            thread_affinity: ThreadAffinity::Main,
            ..Default::default()
        });
        dispatcher.register_handler("main_thread_job", {
            let seen_meta = Arc::clone(&seen_meta);
            move |params| {
                *seen_meta.lock() = params.get("_meta").cloned();
                Ok(json!({"ok": true, "lane": "main"}))
            }
        });

        let (executor, executor_task) = InProcessExecutor.into_handle();
        let registry = Arc::new(registry);
        let catalog = Arc::new(SkillCatalog::new_with_dispatcher(
            Arc::clone(&registry),
            Arc::clone(&dispatcher),
        ));
        let state = ServerState::builder(registry, dispatcher, catalog)
            .with_executor(Some(executor))
            .build();

        let queued = dispatch_rmcp_tool_call(
            &state,
            &ready_context(),
            None,
            "main_thread_job",
            Some(json!({})),
            None,
        )
        .await
        .expect("dispatch should queue async job");
        let job_id = queued
            .structured_content
            .as_ref()
            .and_then(|value| value.get("job_id"))
            .and_then(Value::as_str)
            .expect("pending envelope includes job_id")
            .to_string();

        let mut final_job = None;
        for _ in 0..50 {
            let handle = state.jobs.get(&job_id).expect("job exists");
            let job = handle.read().clone();
            if job.status.is_terminal() {
                final_job = Some(job);
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
        executor_task.abort();

        let job = final_job.expect("job reached terminal state");
        assert_eq!(
            job.status,
            JobStatus::Completed,
            "async main-thread job failed: {:?}",
            job.error
        );
        assert_eq!(job.result, Some(json!({"ok": true, "lane": "main"})));
        let handler_meta = seen_meta
            .lock()
            .clone()
            .expect("handler receives async job metadata");
        assert_eq!(
            handler_meta.pointer("/dcc/jobId").and_then(Value::as_str),
            Some(job_id.as_str())
        );
    }
}
