//! `tools/call` routing for rmcp handlers.

use serde_json::{Value, json};

use dcc_mcp_actions::registry::ToolMeta;
use dcc_mcp_actions::{DispatchError, DispatchResult};
use dcc_mcp_gateway::namespace::{decode_skill_tool_name, extract_bare_tool_name, skill_tool_name};
use dcc_mcp_job::job::{Job, JobStatus};
use dcc_mcp_jsonrpc::{
    CallToolMeta, CallToolResult, DELTA_TOOLS_METHOD, NotificationBuilder, ToolContent,
    coerce_tool_arguments_object,
    error_codes::{BACKEND_NOT_READY, CAPABILITY_MISSING},
};
use dcc_mcp_models::{NextTools, ThreadAffinity};
use dcc_mcp_protocols::error_envelope::DccMcpError;

use crate::dynamic_tools::{
    DYNAMIC_TOOL_PREFIX, build_execution_wrapper, handle_deregister_tool,
    handle_list_dynamic_tools, handle_register_tool,
};
use crate::executor::DccExecutorHandle;
use crate::inflight::CANCEL_GRACE_PERIOD;
use crate::mcp_tool_catalog::{
    action_meta_to_mcp_tool, missing_capabilities, parse_scope_label, resolve_action_by_id,
};
use crate::server_state::ServerState;
use crate::session::SessionManager;

use crate::rmcp_registry_context::RegistryContext;
use crate::rmcp_tool_call_async::{async_dispatch_config, dispatch_async_registry_tool};

// ── notifications (skill load / unload) ─────────────────────────────────────

fn notify_tools_list_changed(sessions: &SessionManager, session_id: &str) {
    let event = NotificationBuilder::new("notifications/tools/list_changed")
        .with_empty_params()
        .as_sse_event();
    sessions.push_event(session_id, event);
}

fn notify_tools_changed(
    sessions: &SessionManager,
    session_id: &str,
    added: &[String],
    removed: &[String],
) {
    if sessions.supports_delta_tools(session_id) {
        let event = NotificationBuilder::new(DELTA_TOOLS_METHOD)
            .with_params(json!({ "added": added, "removed": removed }))
            .as_sse_event();
        sessions.push_event(session_id, event);
    } else {
        notify_tools_list_changed(sessions, session_id);
    }
}

fn attach_next_tools_meta(result: &mut CallToolResult, next_tools: &NextTools) {
    let list = if result.is_error {
        &next_tools.on_failure
    } else {
        &next_tools.on_success
    };
    if list.is_empty() {
        return;
    }
    let key = if result.is_error {
        "on_failure"
    } else {
        "on_success"
    };
    let mut next_tools_meta = serde_json::Map::new();
    next_tools_meta.insert(
        key.to_string(),
        Value::Array(
            list.iter()
                .map(|name| Value::String(name.clone()))
                .collect(),
        ),
    );
    let meta = result.meta.get_or_insert_with(serde_json::Map::new);
    meta.insert("dcc.next_tools".to_string(), Value::Object(next_tools_meta));
}

fn resolve_action_name(state: &ServerState, tool_name: &str) -> String {
    if state.registry.get_action(tool_name, None).is_some() {
        return tool_name.to_string();
    }

    if let Some((skill_part, bare_tool)) = decode_skill_tool_name(tool_name) {
        let matched = state
            .registry
            .list_actions_by_skill(skill_part)
            .into_iter()
            .find(|m| m.name == bare_tool);
        if let Some(m) = matched {
            return m.name;
        }
        return tool_name.to_string();
    }

    let matched = state.registry.list_actions(None).into_iter().find(|m| {
        m.skill_name
            .as_deref()
            .map(|skill_name| extract_bare_tool_name(skill_name, &m.name) == tool_name)
            .unwrap_or(false)
    });
    if let Some(meta) = matched {
        return meta.name;
    }

    tool_name.to_string()
}

fn capability_gate_result(
    state: &ServerState,
    resolved_name: &str,
    action_meta: &ToolMeta,
) -> Option<CallToolResult> {
    let missing = missing_capabilities(
        &action_meta.required_capabilities,
        state.declared_capabilities.as_ref(),
    );
    if missing.is_empty() {
        return None;
    }
    let msg = format!(
        "tool {:?} requires capabilities not advertised by this DCC: {}",
        resolved_name,
        missing.join(", ")
    );
    Some(CallToolResult {
        content: vec![ToolContent::Text { text: msg.clone() }],
        structured_content: Some(json!({
            "code": CAPABILITY_MISSING,
            "message": msg,
            "data": {
                "tool": resolved_name,
                "required_capabilities": action_meta.required_capabilities,
                "declared_capabilities": state.declared_capabilities.as_ref(),
                "missing_capabilities": missing,
            }
        })),
        is_error: true,
        meta: None,
    })
}

fn readiness_gate_result(
    _state: &ServerState,
    ctx: &RegistryContext,
    tool_name: &str,
) -> Option<CallToolResult> {
    let report = ctx.readiness.report();
    if report.is_ready() {
        return None;
    }
    tracing::warn!(
        tool = tool_name,
        readiness.process = report.process,
        readiness.dispatcher = report.dispatcher,
        readiness.dcc = report.dcc,
        "tools/call refused: backend not ready (issue #714)"
    );
    let msg = format!(
        "Backend is not ready yet: process={}, dispatcher={}, dcc={}. \
         Refusing to queue `tools/call` for `{tool_name}` — retry once \
         `/v1/readyz` reports ready.",
        report.process, report.dispatcher, report.dcc
    );
    Some(CallToolResult {
        content: vec![ToolContent::Text { text: msg.clone() }],
        structured_content: Some(json!({
            "code": BACKEND_NOT_READY,
            "message": msg,
            "data": {
                "tool": tool_name,
                "readiness": {
                    "process": report.process,
                    "dispatcher": report.dispatcher,
                    "dcc": report.dcc,
                }
            }
        })),
        is_error: true,
        meta: None,
    })
}

pub(crate) fn use_main_thread_route(
    thread_affinity: ThreadAffinity,
    executor_present: bool,
) -> bool {
    matches!(thread_affinity, ThreadAffinity::Main) && executor_present
}

pub(crate) fn encode_dispatch_wire(result: Result<DispatchResult, DispatchError>) -> String {
    match result {
        Ok(r) => serde_json::to_string(&json!({
            "__dispatch_ok": {
                "action": r.action,
                "output": r.output,
                "validation_skipped": r.validation_skipped,
            }
        }))
        .unwrap_or_else(|_| "{\"__dispatch_ok\":{}}".to_string()),
        Err(err) => encode_dispatch_error_wire(&err),
    }
}

fn encode_dispatch_error_wire(err: &DispatchError) -> String {
    let payload = match err {
        DispatchError::HandlerNotFound(n) => json!({
            "__dispatch_error_kind": "handler_not_found",
            "message": n,
        }),
        DispatchError::MetadataNotFound(n) => json!({
            "__dispatch_error_kind": "metadata_not_found",
            "message": n,
        }),
        DispatchError::ValidationFailed(m) => json!({
            "__dispatch_error_kind": "validation_failed",
            "message": m,
        }),
        DispatchError::HandlerError(m) => json!({
            "__dispatch_error_kind": "handler_error",
            "message": m,
        }),
        DispatchError::ActionDisabled { action, group } => json!({
            "__dispatch_error_kind": "action_disabled",
            "action": action,
            "group": group,
        }),
        DispatchError::ThreadAffinityViolation {
            action,
            declared,
            actual,
        } => json!({
            "__dispatch_error_kind": "thread_affinity_violation",
            "action": action,
            "declared": declared.to_string(),
            "actual": actual.to_string(),
        }),
    };
    serde_json::to_string(&payload).unwrap_or_else(|_| {
        "{\"__dispatch_error_kind\":\"handler_error\",\"message\":\"dispatch failure\"}".to_string()
    })
}

pub(crate) fn decode_dispatch_wire(json_str: &str) -> Result<DispatchResult, DispatchError> {
    let value: Value = serde_json::from_str(json_str).unwrap_or(json!({}));
    if let Some(ok) = value.get("__dispatch_ok") {
        let action = ok
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let output = ok.get("output").cloned().unwrap_or(Value::Null);
        let validation_skipped = ok
            .get("validation_skipped")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return Ok(DispatchResult {
            action,
            output,
            validation_skipped,
        });
    }
    if value.get("__dispatch_error_kind").is_some() {
        return Err(decode_dispatch_error_payload(&value));
    }
    if let Some(err) = value.get("__dispatch_error").and_then(Value::as_str) {
        return Err(DispatchError::HandlerError(err.to_string()));
    }
    Err(DispatchError::HandlerError(
        "malformed dispatch wire payload".to_string(),
    ))
}

fn decode_dispatch_error_payload(value: &Value) -> DispatchError {
    let kind = value
        .get("__dispatch_error_kind")
        .and_then(Value::as_str)
        .unwrap_or("handler_error");
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("dispatch error")
        .to_string();
    match kind {
        "handler_not_found" => DispatchError::HandlerNotFound(message),
        "metadata_not_found" => DispatchError::MetadataNotFound(message),
        "validation_failed" => DispatchError::ValidationFailed(message),
        "handler_error" => DispatchError::HandlerError(message),
        "action_disabled" => DispatchError::ActionDisabled {
            action: value
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            group: value
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        },
        "thread_affinity_violation" => {
            let action = value
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let declared = value
                .get("declared")
                .and_then(Value::as_str)
                .and_then(dcc_mcp_models::ThreadAffinity::parse)
                .unwrap_or(ThreadAffinity::Main);
            let actual = value
                .get("actual")
                .and_then(Value::as_str)
                .and_then(dcc_mcp_models::ThreadAffinity::parse)
                .unwrap_or(ThreadAffinity::Any);
            DispatchError::ThreadAffinityViolation {
                action,
                declared,
                actual,
            }
        }
        _ => DispatchError::HandlerError(message),
    }
}

/// MCP hot path — callers only need the handler output [`Value`].
pub(crate) fn decode_dispatch_output(json_str: &str) -> Result<Value, String> {
    decode_dispatch_wire(json_str)
        .map(|r| r.output)
        .map_err(|e| e.to_string())
}

async fn run_on_main_thread(
    executor: &DccExecutorHandle,
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
) -> Result<DispatchResult, DispatchError> {
    let json_str = executor
        .execute(Box::new(move || {
            encode_dispatch_wire(dcc_mcp_actions::with_thread_affinity(
                ThreadAffinity::Main,
                || dispatcher.dispatch(&resolved_name, call_params),
            ))
        }))
        .await
        .map_err(|e| DispatchError::HandlerError(e.to_string()))?;
    decode_dispatch_wire(&json_str)
}

async fn run_on_worker(
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    resolved_name: String,
    call_params: Value,
) -> Result<DispatchResult, DispatchError> {
    let dispatch_fut =
        tokio::task::spawn_blocking(move || dispatcher.dispatch(&resolved_name, call_params));
    let cancel_wait = async {
        let deadline = tokio::time::Instant::now() + CANCEL_GRACE_PERIOD;
        loop {
            tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            if tokio::time::Instant::now() >= deadline {
                break;
            }
        }
    };
    tokio::select! {
        result = dispatch_fut => result
            .map_err(|err| DispatchError::HandlerError(err.to_string()))?
            ,
        _ = cancel_wait => Err(DispatchError::HandlerError("CANCELLED".to_string())),
    }
}

/// Route a tool dispatch through the same main-thread executor path as MCP
/// `tools/call`. Used by REST `POST /v1/call` via [`crate::ThreadRoutedInvoker`].
pub async fn dispatch_action_with_thread_routing(
    dispatcher: dcc_mcp_actions::ToolDispatcher,
    executor: Option<&DccExecutorHandle>,
    resolved_name: &str,
    call_params: Value,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
) -> Result<DispatchResult, DispatchError> {
    let executor_present = executor.is_some();
    let on_main = use_main_thread_route(thread_affinity, executor_present);

    if matches!(thread_affinity, ThreadAffinity::Main) && !executor_present {
        if enforce_thread_affinity {
            return Err(DispatchError::HandlerError(
                "THREAD_AFFINITY_UNAVAILABLE: tool declares thread_affinity=main, \
                 but no DeferredExecutor is wired"
                    .to_string(),
            ));
        }
        tracing::warn!(
            tool = %resolved_name,
            "sync tool declares thread_affinity=main but no DeferredExecutor is wired; \
             falling back to Tokio worker — scene API calls will be unsafe"
        );
    }

    if on_main {
        let executor = executor.expect("executor presence gated by use_main_thread_route");
        run_on_main_thread(executor, dispatcher, resolved_name.to_string(), call_params).await
    } else {
        run_on_worker(dispatcher, resolved_name.to_string(), call_params).await
    }
}

async fn execute_threaded_dispatch(
    state: &ServerState,
    resolved_name: &str,
    call_params: Value,
    thread_affinity: ThreadAffinity,
    enforce_thread_affinity: bool,
) -> Result<Value, String> {
    dispatch_action_with_thread_routing(
        state.dispatcher.as_ref().clone(),
        state.executor.as_ref(),
        resolved_name,
        call_params,
        thread_affinity,
        enforce_thread_affinity,
    )
    .await
    .map(|r| r.output)
    .map_err(|e| e.to_string())
}

fn dispatch_json_result(output: Value) -> CallToolResult {
    let text = serde_json::to_string(&output).unwrap_or_else(|_| output.to_string());
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(output),
        is_error: false,
        meta: None,
    }
}

fn dispatch_err_result(tool_name: &str, msg: impl Into<String>) -> CallToolResult {
    let err_msg = msg.into();
    if err_msg.contains("no handler registered") {
        let envelope = DccMcpError::new(
            "instance",
            "NO_HANDLER",
            format!("Tool '{tool_name}' is registered but has no handler."),
        )
        .with_hint("Register a handler via ToolDispatcher.register_handler().");
        return CallToolResult::error(envelope.to_json());
    }
    CallToolResult::error(err_msg)
}

// ── Stubs ───────────────────────────────────────────────────────────────────

fn handle_stub_tool(tool_name: &str) -> Option<CallToolResult> {
    if let Some(skill_name) = tool_name.strip_prefix("__skill__") {
        let envelope = DccMcpError::new(
            "gateway",
            "SKILL_NOT_LOADED",
            format!("Skill '{skill_name}' is not loaded."),
        )
        .with_hint(format!(
            "Call load_skill with skill_name=\"{skill_name}\" to register its tools, \
             then call the specific tool you need."
        ));
        return Some(CallToolResult::error(envelope.to_json().to_string()));
    }
    if let Some(group_name) = tool_name.strip_prefix("__group__") {
        let envelope = DccMcpError::new(
            "gateway",
            "GROUP_NOT_ACTIVATED",
            format!("Tool group '{group_name}' is inactive."),
        )
        .with_hint(format!(
            "Call activate_tool_group with group=\"{group_name}\" to enable its tools, \
             then re-list with tools/list."
        ));
        return Some(CallToolResult::error(envelope.to_json().to_string()));
    }
    None
}

// ── Core handlers (sync bodies) ───────────────────────────────────────────

fn handle_list_roots(state: &ServerState, session_id: Option<&str>) -> CallToolResult {
    let Some(session) = session_id else {
        return CallToolResult::error("list_roots requires Mcp-Session-Id header");
    };
    let roots = state.sessions.get_client_roots(session);
    let payload = json!({
        "supports_roots": state.sessions.supports_roots(session),
        "count": roots.len(),
        "roots": roots,
    });
    CallToolResult::text(serde_json::to_string_pretty(&payload).unwrap_or_default())
}

fn handle_list_skills(state: &ServerState, arguments: &Value) -> CallToolResult {
    let status = arguments.get("status").and_then(Value::as_str);
    let results = state.catalog.list_skills(status);
    let payload =
        dcc_mcp_skills::catalog::list_projection::build_list_skills_response(results, arguments);
    let text = serde_json::to_string_pretty(&payload).unwrap_or_default();
    CallToolResult::text(text)
}

fn handle_get_skill_info(state: &ServerState, arguments: &Value) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if skill_name.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name");
    }
    match state.catalog.get_skill_info(skill_name) {
        Some(info) => {
            let text = serde_json::to_string_pretty(&info).unwrap_or_default();
            CallToolResult::text(text)
        }
        None => CallToolResult::error(format!("Skill '{skill_name}' not found")),
    }
}

fn handle_load_skill(
    state: &ServerState,
    ctx: &RegistryContext,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    let skill_names: Vec<String> = arguments
        .get("skill_names")
        .and_then(Value::as_array)
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();

    if skill_name.is_empty() && skill_names.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name or skill_names");
    }

    let activate_groups = arguments
        .get("activate_groups")
        .and_then(Value::as_bool)
        .unwrap_or(true);

    let mut requested: Vec<String> = Vec::new();
    if !skill_name.is_empty() {
        requested.push(skill_name.to_string());
    }
    for name in &skill_names {
        if !requested.contains(name) {
            requested.push(name.clone());
        }
    }

    let mut all_registered_tools: Vec<String> = Vec::new();
    let mut errors: Vec<String> = Vec::new();
    let mut newly_loaded: Vec<String> = Vec::new();
    let mut already_loaded: Vec<String> = Vec::new();

    for name in &requested {
        let was_loaded = state.catalog.is_loaded(name);
        match state.catalog.load_skill_with_options(name, activate_groups) {
            Ok(tools) => {
                all_registered_tools.extend(tools);
                if was_loaded {
                    already_loaded.push(name.clone());
                } else {
                    newly_loaded.push(name.clone());
                }
            }
            Err(e) => errors.push(format!("{name}: {e}")),
        }
    }

    if !newly_loaded.is_empty() {
        state.bump_registry_generation();
        if let Some(sid) = session_id {
            let added = all_registered_tools.clone();
            let removed: Vec<String> = newly_loaded
                .iter()
                .map(|n| format!("__skill__{n}"))
                .collect();
            notify_tools_changed(&state.sessions, sid, &added, &removed);
        }
        (ctx.on_skill_catalog_mutated)();
    }

    let mut tool_schemas: Vec<Value> = Vec::new();
    for name in newly_loaded.iter().chain(already_loaded.iter()) {
        for meta in state.catalog.registry().list_actions_by_skill(name) {
            tool_schemas.push(json!({
                "name":          meta.name,
                "description":   meta.description,
                "inputSchema":   meta.input_schema,
                "outputSchema":  meta.output_schema,
                "skill_name":    meta.skill_name,
            }));
        }
    }

    let loaded_ok = !all_registered_tools.is_empty();
    let partial = loaded_ok && !errors.is_empty();

    let mut body = json!({
        "loaded":           loaded_ok,
        "partial":          partial,
        "registered_tools": all_registered_tools,
        "tool_count":       all_registered_tools.len(),
        "newly_loaded":     newly_loaded,
        "already_loaded":   already_loaded,
        "tools":            tool_schemas,
    });
    if !errors.is_empty()
        && let Some(obj) = body.as_object_mut()
    {
        obj.insert("errors".to_string(), json!(errors));
    }

    let text = serde_json::to_string_pretty(&body).unwrap_or_default();
    if loaded_ok {
        CallToolResult::text(text)
    } else {
        CallToolResult::error(text)
    }
}

fn handle_unload_skill(
    state: &ServerState,
    ctx: &RegistryContext,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let skill_name = arguments
        .get("skill_name")
        .and_then(Value::as_str)
        .unwrap_or_default();
    if skill_name.is_empty() {
        return CallToolResult::error("Missing required parameter: skill_name");
    }

    match state.catalog.unload_skill(skill_name) {
        Ok(count) => {
            state.bump_registry_generation();
            if let Some(sid) = session_id {
                let removed: Vec<String> = state
                    .registry
                    .list_actions_by_skill(skill_name)
                    .iter()
                    .map(|m| m.name.clone())
                    .collect();
                let added = vec![format!("__skill__{skill_name}")];
                notify_tools_changed(&state.sessions, sid, &added, &removed);
            }
            (ctx.on_skill_catalog_mutated)();
            let text = serde_json::to_string_pretty(&json!({
                "unloaded": true,
                "tools_removed": count
            }))
            .unwrap_or_default();
            CallToolResult::text(text)
        }
        Err(e) => CallToolResult::error(e),
    }
}

fn handle_search_skills(state: &ServerState, arguments: &Value) -> CallToolResult {
    const DEFAULT_LIMIT: usize = 20;
    const MAX_LIMIT: usize = 100;

    let query = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default();

    let tags_owned: Vec<String> = arguments
        .get("tags")
        .and_then(|t| t.as_array())
        .map(|arr| {
            arr.iter()
                .filter_map(Value::as_str)
                .map(String::from)
                .collect()
        })
        .unwrap_or_default();
    let tags: Vec<&str> = tags_owned.iter().map(String::as_str).collect();

    let dcc_filter = arguments.get("dcc").and_then(Value::as_str);

    let scope_filter = match arguments.get("scope").and_then(Value::as_str) {
        None => None,
        Some(s) => match parse_scope_label(s) {
            Ok(sc) => Some(sc),
            Err(msg) => return CallToolResult::error(msg),
        },
    };

    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n as usize)
        .unwrap_or(DEFAULT_LIMIT)
        .clamp(1, MAX_LIMIT);

    let query_opt = if query.is_empty() { None } else { Some(query) };
    let matches =
        state
            .catalog
            .search_skills(query_opt, &tags, dcc_filter, scope_filter, Some(limit));

    if matches.is_empty() {
        let text = if query.is_empty()
            && tags.is_empty()
            && dcc_filter.is_none()
            && scope_filter.is_none()
        {
            "No skills discovered. Drop SKILL.md files into the scan paths and rescan.".to_string()
        } else if query.is_empty() {
            "No skills match the given filters.".to_string()
        } else {
            format!("No skills found matching '{query}'.")
        };
        return CallToolResult::text(text);
    }

    let compact_skills: Vec<Value> = matches
        .iter()
        .map(|s| {
            json!({
                "name": s.name,
                "description": s.description,
                "tools": s.tool_count,
                "loaded": s.loaded,
                "dcc": s.dcc,
                "scope": s.scope,
                "tags": s.tags,
                "search_hint": s.search_hint,
            })
        })
        .collect();

    let result = json!({
        "total": matches.len(),
        "query": query,
        "skills": compact_skills
    });

    CallToolResult::text(serde_json::to_string(&result).unwrap_or_default())
}

fn handle_activate_tool_group(
    state: &ServerState,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let group = arguments
        .get("group")
        .or_else(|| arguments.get("group_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return CallToolResult::error("Missing required parameter: group or group_name");
    }
    let changed = state.catalog.activate_group(group);
    state.bump_registry_generation();
    if let Some(sid) = session_id {
        let added: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let removed = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    CallToolResult::text(
        json!({
            "success": true,
            "group": group,
            "changed": changed,
            "active_groups": state.catalog.active_groups(),
        })
        .to_string(),
    )
}

fn handle_deactivate_tool_group(
    state: &ServerState,
    arguments: &Value,
    session_id: Option<&str>,
) -> CallToolResult {
    let group = arguments
        .get("group")
        .or_else(|| arguments.get("group_name"))
        .and_then(Value::as_str)
        .unwrap_or_default();
    if group.is_empty() {
        return CallToolResult::error("Missing required parameter: group or group_name");
    }
    let changed = state.catalog.deactivate_group(group);
    state.bump_registry_generation();
    if let Some(sid) = session_id {
        let removed: Vec<String> = state
            .registry
            .list_actions_in_group(group)
            .iter()
            .map(|m| m.name.clone())
            .collect();
        let added = vec![format!("__group__{group}")];
        notify_tools_changed(&state.sessions, sid, &added, &removed);
    }
    CallToolResult::text(
        json!({
            "success": true,
            "group": group,
            "changed": changed,
            "active_groups": state.catalog.active_groups(),
        })
        .to_string(),
    )
}

fn is_progressive_stub(name: &str) -> bool {
    crate::mcp_tool_catalog::is_progressive_tool_stub(name)
}

fn schema_property_names(schema: &Value) -> Vec<String> {
    schema
        .get("properties")
        .and_then(Value::as_object)
        .map(|props| props.keys().cloned().collect())
        .unwrap_or_default()
}

fn handle_search_tools(state: &ServerState, arguments: &Value) -> CallToolResult {
    let query_raw = arguments
        .get("query")
        .and_then(Value::as_str)
        .unwrap_or_default()
        .trim();
    if query_raw.is_empty() {
        return CallToolResult::error("Missing required parameter: query");
    }
    let query = query_raw.to_lowercase();

    let dcc = arguments.get("dcc").and_then(Value::as_str);
    let include_disabled = arguments
        .get("include_disabled")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_stubs = arguments
        .get("include_stubs")
        .and_then(Value::as_bool)
        .unwrap_or(false);
    let include_unloaded_skills = arguments
        .get("include_unloaded_skills")
        .and_then(Value::as_bool)
        .unwrap_or(true);
    let limit = arguments
        .get("limit")
        .and_then(Value::as_u64)
        .map(|n| n.clamp(1, 100) as usize)
        .unwrap_or(25);

    let mut tool_hits: Vec<Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !include_disabled && !meta.enabled {
            continue;
        }
        if !include_stubs && is_progressive_stub(&meta.name) {
            continue;
        }
        let schema_props = schema_property_names(&meta.input_schema);
        let haystack = format!(
            "{} {} {} {} {}",
            meta.name,
            meta.description,
            meta.category,
            meta.tags.join(" "),
            schema_props.join(" ")
        )
        .to_lowercase();
        if !haystack.contains(&query) {
            continue;
        }
        let mut hit = json!({
            "kind": "tool",
            "name": meta.name,
            "description": meta.description,
            "category": meta.category,
            "group": meta.group,
            "enabled": meta.enabled,
            "dcc": meta.dcc,
        });
        if let Some(skill) = &meta.skill_name {
            hit["skill_name"] = Value::String(skill.clone());
        }
        tool_hits.push(hit);
        if tool_hits.len() >= limit {
            break;
        }
    }

    if include_stubs && tool_hits.len() < limit {
        for summary in state.catalog.list_skills(Some("unloaded")) {
            if let Some(filter) = dcc
                && !summary.dcc.eq_ignore_ascii_case(filter)
            {
                continue;
            }
            let haystack = format!(
                "{} {} {} {} {}",
                summary.name,
                summary.description,
                summary.search_hint,
                summary.tags.join(" "),
                summary.tool_names.join(" ")
            )
            .to_lowercase();
            if !haystack.contains(&query) {
                continue;
            }
            tool_hits.push(json!({
                "kind": "tool",
                "name": format!("__skill__{}", summary.name),
                "description": format!(
                    "[stub] unloaded skill `{}` — call load_skill(\"{}\") to expose its {} tool(s)",
                    summary.name, summary.name, summary.tool_count,
                ),
                "category": "stub",
                "group": "",
                "enabled": false,
                "dcc": summary.dcc,
                "skill_name": summary.name,
            }));
            if tool_hits.len() >= limit {
                break;
            }
        }

        if tool_hits.len() < limit {
            let mut seen_groups: std::collections::HashSet<String> =
                std::collections::HashSet::new();
            for (skill, group, active) in state.catalog.list_groups() {
                if active {
                    continue;
                }
                if !seen_groups.insert(group.clone()) {
                    continue;
                }
                let haystack = format!("__group__{} {} {}", group, group, skill).to_lowercase();
                if !haystack.contains(&query) {
                    continue;
                }
                tool_hits.push(json!({
                    "kind": "tool",
                    "name": format!("__group__{}", group),
                    "description": format!(
                        "[stub] inactive tool group `{}` — call activate_tool_group(group=\"{}\") to expose its members",
                        group, group,
                    ),
                    "category": "stub",
                    "group": group,
                    "enabled": false,
                    "dcc": "",
                    "skill_name": skill,
                }));
                if tool_hits.len() >= limit {
                    break;
                }
            }
        }
    }

    let mut skill_candidates: Vec<Value> = Vec::new();
    if include_unloaded_skills {
        let candidates = state
            .catalog
            .search_skills(Some(query_raw), &[], dcc, None, Some(limit));
        for summary in candidates {
            if summary.loaded {
                continue;
            }
            let detail = state.catalog.get_skill_info(&summary.name);
            let matching_tools = detail
                .as_ref()
                .map(|d| {
                    d.tools
                        .iter()
                        .filter(|t| {
                            t.name.to_lowercase().contains(&query)
                                || t.description.to_lowercase().contains(&query)
                        })
                        .map(|t| t.name.clone())
                        .collect::<Vec<_>>()
                })
                .unwrap_or_default();
            skill_candidates.push(json!({
                "kind": "skill_candidate",
                "skill_name": summary.name,
                "description": summary.description,
                "tags": summary.tags,
                "dcc": summary.dcc,
                "scope": summary.scope,
                "tool_count": summary.tool_count,
                "matching_tools": matching_tools,
                "requires_load_skill": true,
                "load_hint": {
                    "tool": "load_skill",
                    "arguments": { "skill_name": summary.name },
                },
            }));
        }
    }

    let total = tool_hits.len() + skill_candidates.len();
    let result = json!({
        "total": total,
        "query": query,
        "tools": tool_hits,
        "skill_candidates": skill_candidates,
    });
    CallToolResult::text(serde_json::to_string(&result).unwrap_or_default())
}

fn compute_job_timestamps(
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

fn handle_jobs_get_status(state: &ServerState, arguments: &Value) -> CallToolResult {
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

fn handle_jobs_cleanup(state: &ServerState, arguments: &Value) -> CallToolResult {
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

fn handle_list_actions(state: &ServerState, arguments: &Value) -> CallToolResult {
    let dcc = arguments.get("dcc").and_then(Value::as_str);
    let skill_filter = arguments.get("skill").and_then(Value::as_str);

    let mut items: Vec<Value> = Vec::new();
    for meta in state.registry.list_actions(dcc) {
        if !meta.enabled {
            continue;
        }
        if let Some(want) = skill_filter
            && meta.skill_name.as_deref() != Some(want)
        {
            continue;
        }
        let id = meta
            .skill_name
            .as_deref()
            .and_then(|sn| skill_tool_name(sn, &meta.name))
            .unwrap_or_else(|| meta.name.clone());
        items.push(json!({
            "id": id,
            "summary": meta.description,
            "tags": meta.tags,
        }));
    }

    let payload = json!({
        "total": items.len(),
        "actions": items,
    });
    CallToolResult::text(serde_json::to_string(&payload).unwrap_or_default())
}

fn handle_describe_action(
    state: &ServerState,
    arguments: &Value,
    _session_id: Option<&str>,
) -> CallToolResult {
    let id = match arguments.get("id").and_then(Value::as_str) {
        Some(s) if !s.is_empty() => s.to_string(),
        _ => return CallToolResult::error("Missing required parameter: id"),
    };

    let Some(meta) = resolve_action_by_id(&state.registry, &id) else {
        let envelope = DccMcpError::new(
            "registry",
            "ACTION_NOT_FOUND",
            format!("Unknown action id: {id}"),
        )
        .with_hint("Call list_actions to see available ids.");
        return CallToolResult::error(envelope.to_json().to_string());
    };

    let include_output_schema = true;
    let bare_eligible_for_describe = std::collections::HashSet::new();
    let tool = action_meta_to_mcp_tool(
        &meta,
        include_output_schema,
        &bare_eligible_for_describe,
        state.declared_capabilities.as_ref(),
    );
    let payload = serde_json::to_value(tool).unwrap_or_default();
    CallToolResult::text(serde_json::to_string(&payload).unwrap_or_default())
}

fn handle_register_tool_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
    arguments: &Value,
) -> CallToolResult {
    let sid = match session_id {
        Some(id) => id,
        None => {
            return CallToolResult::error(
                "register_tool requires an active session (send Mcp-Session-Id header)",
            );
        }
    };

    let result_value = state
        .sessions
        .with_dynamic_tools_mut(sid, |dyn_tools| handle_register_tool(dyn_tools, arguments))
        .unwrap_or_else(|| {
            json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Session not found" }]
            })
        });

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

fn handle_deregister_tool_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
    arguments: &Value,
) -> CallToolResult {
    let sid = match session_id {
        Some(id) => id,
        None => return CallToolResult::error("deregister_tool requires an active session"),
    };

    let result_value = state
        .sessions
        .with_dynamic_tools_mut(sid, |dyn_tools| {
            handle_deregister_tool(dyn_tools, arguments)
        })
        .unwrap_or_else(|| {
            json!({
                "isError": true,
                "content": [{ "type": "text", "text": "Session not found" }]
            })
        });

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

fn handle_list_dynamic_tools_dynamic(
    state: &ServerState,
    session_id: Option<&str>,
) -> CallToolResult {
    let result_value = if let Some(sid) = session_id {
        state
            .sessions
            .with_dynamic_tools_mut(sid, handle_list_dynamic_tools)
            .unwrap_or_else(|| {
                json!({
                    "content": [{ "type": "text", "text": "{\"dynamic_tools\":[], \"count\":0}" }]
                })
            })
    } else {
        handle_list_dynamic_tools(&mut crate::dynamic_tools::SessionDynamicTools::new())
    };

    let text = serde_json::to_string(&result_value).unwrap_or_default();
    CallToolResult {
        content: vec![ToolContent::Text { text }],
        structured_content: Some(result_value.clone()),
        is_error: result_value
            .get("isError")
            .and_then(Value::as_bool)
            .unwrap_or(false),
        meta: None,
    }
}

fn route_dynamic_execution(
    state: &ServerState,
    session_id: Option<&str>,
    tool_name: &str,
    arguments: Value,
) -> Option<CallToolResult> {
    let sid = session_id?;

    let spec_opt = state.sessions.with_dynamic_tools_mut(sid, |dyn_tools| {
        dyn_tools.get(tool_name).map(|e| e.spec.clone())
    });
    let spec = spec_opt.flatten()?;

    let wrapper_script = build_execution_wrapper(&spec.name, &spec.code, &arguments);

    if state.executor.is_none() {
        let payload = json!({
            "status": "no_executor",
            "message": format!(
                "Dynamic tool '{}' is registered but requires an in-process \
                    DeferredExecutor to execute. Wire DeferredExecutor via \
                    McpHttpConfig::set_executor() in your DCC adapter.",
                spec.name
            ),
            "generated_script": wrapper_script,
            "call_args": arguments
        });
        return Some(CallToolResult::text(
            serde_json::to_string(&payload).unwrap_or_default(),
        ));
    }

    let payload = json!({
        "status": "pending_stage2",
        "message": format!(
            "Dynamic tool '{}' execution queued. Full in-process Python \
                evaluation (Stage 2, issue #462) is not yet implemented. \
                The DCC adapter must provide a PythonEvalHandler.",
            spec.name
        ),
        "generated_script": wrapper_script,
        "call_args": arguments
    });
    Some(CallToolResult::text(
        serde_json::to_string(&payload).unwrap_or_default(),
    ))
}

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
        "jobs.get_status" => Ok(handle_jobs_get_status(state, &arguments_value)),
        "jobs.cleanup" => Ok(handle_jobs_cleanup(state, &arguments_value)),
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
