//! `tools/call` routing for rmcp handlers.

mod handlers;
mod helpers;
mod thread_route;
mod wire;

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
    use dcc_mcp_models::{ExecutionMode, ThreadAffinity};
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
    async fn async_main_thread_job_decodes_deferred_dispatch_wire() {
        let registry = ToolRegistry::new();
        let dispatcher = Arc::new(ToolDispatcher::new(registry.clone()));

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
        dispatcher.register_handler("main_thread_job", |_| {
            Ok(json!({"ok": true, "lane": "main"}))
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
    }
}
