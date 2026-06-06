//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use crate::gateway::admin::trace::{AgentContext, TraceContext};
use crate::gateway::capability_service::{SearchResponseContext, search_hit_to_value_with_context};
use crate::gateway::search_telemetry::{
    RANKER_VERSION, SearchFollowupInput, SearchTelemetryHit, SearchTelemetryInput,
    search_id_from_meta, search_id_from_payload,
};

use super::state::GatewayState;
use dcc_mcp_jsonrpc::coerce_tool_arguments_object;
use dcc_mcp_transport::discovery::types::ServiceKey;

// ── tools ──────────────────────────────────────────────────────────────────

/// `acquire_dcc_instance` — reserve an idle DCC instance for a workflow/client.
pub async fn tool_acquire_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_type = args
        .get("dcc_type")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide dcc_type".to_string())?;
    let owner = args
        .get("lease_owner")
        .and_then(|v| v.as_str())
        .unwrap_or("anonymous");
    let instance_id = args.get("instance_id").and_then(|v| v.as_str());
    let current_job_id = args
        .get("current_job_id")
        .and_then(|v| v.as_str())
        .map(str::to_owned);
    let ttl_secs = args
        .get("ttl_secs")
        .and_then(|v| v.as_u64())
        .unwrap_or(3600)
        .max(1);

    let reg = gs.registry.read().await;
    let resolved_instance_id = if instance_id.is_some() {
        Some(
            gs.resolve_instance(&reg, instance_id, Some(dcc_type))
                .map_err(|err| err.to_string())?
                .instance_id
                .to_string(),
        )
    } else {
        None
    };
    let Some(entry) = reg
        .acquire_lease(
            dcc_type,
            resolved_instance_id.as_deref(),
            owner,
            current_job_id,
            Some(std::time::Duration::from_secs(ttl_secs)),
        )
        .map_err(|e| e.to_string())?
    else {
        return Err(format!(
            "No idle '{dcc_type}' instance is available for lease. \
             Release a busy instance or start another DCC process."
        ));
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Leased {dcc_type} instance {}", entry.instance_id),
        "instance": gs.instance_json(&entry),
    }))
    .map_err(|e| e.to_string())
}

/// `release_dcc_instance` — release a previously acquired instance lease.
pub async fn tool_release_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let instance_id = args
        .get("instance_id")
        .and_then(|v| v.as_str())
        .ok_or_else(|| "Provide instance_id".to_string())?;
    let owner = args.get("lease_owner").and_then(|v| v.as_str());

    let reg = gs.registry.read().await;

    let entry = gs
        .resolve_instance(&reg, Some(instance_id), None)
        .map_err(|err| err.to_string())?;
    let key = ServiceKey {
        dcc_type: entry.dcc_type.clone(),
        instance_id: entry.instance_id,
    };

    let Some(row) = reg.get(&key) else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "unknown_instance",
            "message": format!("No FileRegistry row for instance_id {instance_id} after resolve"),
        }))
        .unwrap_or_else(|_| "unknown_instance".to_string()));
    };

    match row.lease_owner.as_deref() {
        None => {
            return Err(serde_json::to_string_pretty(&json!({
                "success": false,
                "reason": "no_active_lease",
                "message": "This instance has no active pool lease in the shared registry.",
                "hint": "Call acquire_dcc_instance first (same lease_owner string you plan to pass to release). release_dcc_instance only clears pool metadata in services.json — it does not close Maya or drop MCP connections.",
                "instance_id": entry.instance_id.to_string(),
                "instance": gs.instance_json(&entry),
            }))
            .unwrap_or_else(|_| "no_active_lease".to_string()));
        }
        Some(current) => {
            if let Some(expected) = owner
                && expected != current
            {
                return Err(serde_json::to_string_pretty(&json!({
                    "success": false,
                    "reason": "lease_owner_mismatch",
                    "message": format!(
                        "lease_owner {expected:?} does not match the active lease holder {current:?}"
                    ),
                    "hint": "Omit lease_owner on release to clear any lease, or pass the exact string used in acquire_dcc_instance.",
                    "instance_id": entry.instance_id.to_string(),
                    "active_lease_owner": current,
                }))
                .unwrap_or_else(|_| "lease_owner_mismatch".to_string()));
            }
        }
    }

    let Some(released) = reg.release_lease(&key, owner).map_err(|e| e.to_string())? else {
        return Err(serde_json::to_string_pretty(&json!({
            "success": false,
            "reason": "release_rejected",
            "message": "Registry refused to clear the lease after pre-flight checks — possible concurrent mutation; retry once.",
            "instance_id": entry.instance_id.to_string(),
        }))
        .unwrap_or_else(|_| "release_rejected".to_string()));
    };

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": format!("Released lease for instance {}", released.instance_id),
        "instance": gs.instance_json(&released),
    }))
    .map_err(|e| e.to_string())
}

// ── Gateway MCP tools ────────────────────────────────────────────────────

/// Unified search: backend capabilities (`kind=tool`, default) or skills (`kind=skill`).
pub async fn tool_search(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    session_id: Option<&str>,
    agent_context: Option<&AgentContext>,
) -> Result<String, String> {
    let kind = args
        .get("kind")
        .and_then(Value::as_str)
        .unwrap_or("tool")
        .to_ascii_lowercase();
    match kind.as_str() {
        "skill" | "skills" => {
            let has_query = args
                .get("query")
                .and_then(Value::as_str)
                .is_some_and(|q| !q.trim().is_empty());
            let legacy = if has_query {
                "search_skills"
            } else {
                "list_skills"
            };
            let (text, is_error) =
                crate::gateway::aggregator::skill_mgmt_dispatch(gs, legacy, args).await;
            if is_error {
                Err(text)
            } else {
                Ok(annotate_skill_search_payload(
                    gs,
                    args,
                    &text,
                    trace_context,
                    session_id,
                    agent_context,
                ))
            }
        }
        "all" => {
            let tools_json =
                tool_search_tools(gs, args, trace_context, session_id, agent_context).await?;
            let (skills_text, skills_err) =
                crate::gateway::aggregator::skill_mgmt_dispatch(gs, "list_skills", args).await;
            if skills_err {
                return Err(skills_text);
            }
            let tools_value = serde_json::from_str::<Value>(&tools_json).unwrap_or(Value::Null);
            let skills_json = annotate_skill_search_payload(
                gs,
                args,
                &skills_text,
                trace_context,
                session_id,
                agent_context,
            );
            let skills_value = serde_json::from_str::<Value>(&skills_json).unwrap_or(Value::Null);
            let search_id = tools_value
                .get("search_id")
                .or_else(|| skills_value.get("search_id"))
                .cloned()
                .unwrap_or(Value::Null);
            let ranker_version = tools_value
                .get("ranker_version")
                .or_else(|| skills_value.get("ranker_version"))
                .cloned()
                .unwrap_or_else(|| json!(RANKER_VERSION));
            let index_generation = tools_value
                .get("index_generation")
                .or_else(|| skills_value.get("index_generation"))
                .cloned()
                .unwrap_or(Value::Null);
            Ok(serde_json::to_string_pretty(&json!({
                "search_id": search_id,
                "ranker_version": ranker_version,
                "index_generation": index_generation,
                "tools": tools_value,
                "skills": skills_value,
            }))
            .map_err(|e| e.to_string())?)
        }
        _ => {
            let _ = meta;
            tool_search_tools(gs, args, trace_context, session_id, agent_context).await
        }
    }
}

/// Unified describe: `tool_slug` for backend schema, or `skill_name` for skill detail.
pub async fn tool_describe(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
) -> Result<String, String> {
    if args.get("tool_slug").and_then(Value::as_str).is_some() {
        return tool_describe_tool(gs, args, meta, trace_context).await;
    }
    if args.get("skill_name").and_then(Value::as_str).is_some() {
        let (text, is_error) =
            crate::gateway::aggregator::skill_mgmt_dispatch(gs, "get_skill_info", args).await;
        record_search_followup(
            gs,
            search_id_from_inputs(args, meta).as_deref(),
            "describe",
            None,
            skill_name_from_payload(args),
            !is_error,
            trace_context,
        );
        if is_error { Err(text) } else { Ok(text) }
    } else {
        Err("describe requires `tool_slug` (from search) or `skill_name`".to_string())
    }
}

/// Unified call: single `tool_slug` or ordered `calls` batch (same shape as legacy wrappers).
pub async fn tool_call(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    agent_context: Option<&AgentContext>,
) -> (String, bool) {
    if args.get("calls").and_then(Value::as_array).is_some() {
        tool_call_tools(gs, args, meta, trace_context, agent_context).await
    } else {
        tool_call_tool(gs, args, meta, trace_context, agent_context).await
    }
}

/// Instance pooling: `action` = `acquire` (default) or `release`.
pub async fn tool_lease(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let action = args
        .get("action")
        .and_then(Value::as_str)
        .unwrap_or("acquire");
    if action.eq_ignore_ascii_case("release") {
        tool_release_instance(gs, args).await
    } else {
        tool_acquire_instance(gs, args).await
    }
}

/// Load a skill and optionally activate/deactivate a progressive tool group.
pub async fn tool_load_skill(gs: &GatewayState, args: &Value) -> (String, bool) {
    let group_action = args
        .get("group_action")
        .and_then(Value::as_str)
        .map(|s| s.to_ascii_lowercase());
    let tool_group = args
        .get("tool_group")
        .or_else(|| args.get("group_name"))
        .cloned();

    if matches!(group_action.as_deref(), Some("deactivate")) {
        let mut forward = args.clone();
        if let Some(obj) = forward.as_object_mut() {
            if let Some(g) = tool_group {
                obj.insert("group_name".to_string(), g);
            }
            obj.remove("tool_group");
            obj.remove("group_action");
        }
        return crate::gateway::aggregator::skill_mgmt_dispatch(
            gs,
            "deactivate_tool_group",
            &forward,
        )
        .await;
    }

    if tool_group.is_some() && matches!(group_action.as_deref(), Some("activate") | None) {
        if args.get("skill_name").and_then(Value::as_str).is_some() {
            let (load_text, load_err) =
                crate::gateway::aggregator::skill_mgmt_dispatch(gs, "load_skill", args).await;
            if load_err {
                return (load_text, true);
            }
            let mut group_args = args.clone();
            if let Some(obj) = group_args.as_object_mut() {
                if let Some(g) = tool_group {
                    obj.insert("group_name".to_string(), g);
                }
                obj.remove("tool_group");
                obj.remove("group_action");
            }
            let (group_text, group_err) = crate::gateway::aggregator::skill_mgmt_dispatch(
                gs,
                "activate_tool_group",
                &group_args,
            )
            .await;
            if group_err {
                return (group_text, true);
            }
            let combined = format!("{load_text}\n{group_text}");
            return (combined, false);
        }
        let mut forward = args.clone();
        if let Some(obj) = forward.as_object_mut() {
            if let Some(g) = tool_group {
                obj.insert("group_name".to_string(), g);
            }
            obj.remove("tool_group");
            obj.remove("group_action");
        }
        return crate::gateway::aggregator::skill_mgmt_dispatch(
            gs,
            "activate_tool_group",
            &forward,
        )
        .await;
    }

    crate::gateway::aggregator::skill_mgmt_dispatch(gs, "load_skill", args).await
}

// ── #655 dynamic-capability MCP wrappers ──────────────────────────────────

/// `search_tools` — MCP wrapper that routes to
/// [`crate::gateway::capability_service::search_service`].
///
/// Kept alongside the REST handler so both transports produce
/// byte-identical responses for the same query.
pub async fn tool_search_tools(
    gs: &GatewayState,
    args: &Value,
    trace_context: Option<&TraceContext>,
    session_id: Option<&str>,
    agent_context: Option<&AgentContext>,
) -> Result<String, String> {
    // Refresh on demand so the first query after startup (or after
    // a skill load/unload) always sees current capabilities.
    crate::gateway::capability_service::refresh_all_live_backends(
        gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let query = crate::gateway::capability_service::parse_search_payload(args);
    let index_generation =
        crate::gateway::capability_service::index_generation(&gs.capability_index);
    let search_context = SearchResponseContext::new(
        crate::gateway::search_telemetry::SearchTelemetryStore::new_search_id(),
        index_generation,
    );
    let hits = crate::gateway::capability_service::search_service_hits_for_policy(
        &gs.capability_index,
        &query,
        &gs.policy,
    );
    let telemetry_hits = search_hits_for_telemetry(&hits);
    let annotated: Vec<Value> = hits
        .into_iter()
        .map(|hit| search_hit_to_value_with_context(hit, Some(&search_context)))
        .collect();
    gs.search_telemetry.record_search(SearchTelemetryInput {
        search_id: search_context.search_id.clone(),
        transport: "mcp".to_string(),
        kind: "tool".to_string(),
        query: query.query.clone(),
        dcc_type: query.dcc_type.clone(),
        instance_id: query.instance_id.map(|id| id.to_string()),
        limit: query.limit,
        total: annotated.len(),
        ranker_version: search_context.ranker_version.to_string(),
        index_generation: search_context.index_generation.clone(),
        hits: telemetry_hits,
        trace_context: trace_context.cloned(),
        session_id: session_id
            .map(str::to_string)
            .or_else(|| agent_context.and_then(|ctx| ctx.session_id.clone())),
        agent_context: agent_context.cloned(),
    });

    serde_json::to_string_pretty(&json!({
        "search_id": search_context.search_id,
        "ranker_version": search_context.ranker_version,
        "index_generation": search_context.index_generation,
        "total": annotated.len(),
        "hits":  annotated,
    }))
    .map_err(|e| e.to_string())
}

/// `describe_tool` — MCP wrapper around
/// [`crate::gateway::capability_service::describe_service`].
pub async fn tool_describe_tool(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
) -> Result<String, String> {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return Err("missing required argument: tool_slug".to_string());
    };
    if describe_needs_refresh(gs, slug, args, meta) {
        crate::gateway::capability_service::refresh_all_live_backends(
            gs,
            crate::gateway::capability::RefreshReason::Periodic,
        )
        .await;
    }
    let search_id = search_id_from_inputs(args, meta);
    match crate::gateway::capability_service::describe_tool_full(gs, slug).await {
        Ok((record, tool)) => {
            record_search_followup(
                gs,
                search_id.as_deref(),
                "describe",
                Some(slug),
                None,
                true,
                trace_context,
            );
            let input_schema = tool.input_schema.clone();
            let required = input_schema
                .get("required")
                .cloned()
                .unwrap_or_else(|| json!([]));
            let properties = input_schema.get("properties").cloned();
            let mut payload = json!({
                "record": record,
                "tool": tool,
                "input_schema": input_schema,
                "required": required,
                "properties": properties,
                "hint": "Copy parameter names from `properties` / `required` into call.arguments (e.g. export_fbx uses `path`, not `destination`).",
            });
            if let Some(search_id) = search_id.as_deref() {
                payload["next_step"] = call_next_step(slug, search_id);
            }
            serde_json::to_string_pretty(&payload).map_err(|e| e.to_string())
        }
        Err(err) => {
            record_search_followup(
                gs,
                search_id.as_deref(),
                "describe",
                Some(slug),
                None,
                false,
                trace_context,
            );
            let payload = crate::gateway::capability_service::service_error_to_json(&err);
            Err(serde_json::to_string_pretty(&payload).unwrap_or_else(|_| err.message.clone()))
        }
    }
}

/// `call_tool` — MCP wrapper around
/// [`crate::gateway::capability_service::call_service`].
///
/// Returns the raw backend `tools/call` envelope on success so
/// progress events and structured content survive the wrapper.
pub async fn tool_call_tool(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    agent_context: Option<&AgentContext>,
) -> (String, bool) {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return ("missing required argument: tool_slug".to_string(), true);
    };
    let arguments = match coerce_tool_arguments_object(args.get("arguments").cloned()) {
        Ok(v) => v,
        Err(msg) => return (msg, true),
    };
    let forwarded_meta = args.get("meta").cloned().or_else(|| meta.cloned());
    let search_id = search_id_from_inputs(args, meta);
    // No refresh here on purpose: `call_tool` is the hot path and
    // we trust that the caller used `describe_tool` / `search_tools`
    // to obtain the slug, both of which refresh. An unknown-slug
    // error from `describe_service` will trigger one refresh at the
    // end of this function if the record is missing, keeping the
    // happy path fast.
    match crate::gateway::capability_service::call_service(
        gs,
        slug,
        arguments.clone(),
        forwarded_meta.clone(),
        trace_context,
        agent_context,
    )
    .await
    {
        Ok(result) => {
            record_search_followup(
                gs,
                search_id.as_deref(),
                "call",
                Some(slug),
                None,
                true,
                trace_context,
            );
            (
                serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
                false,
            )
        }
        Err(err) if err.kind == "unknown-slug" => {
            // Refresh once in case the caller supplied a slug that
            // just became valid (e.g. a skill loaded between
            // `search_tools` and `call_tool`), then retry.
            crate::gateway::capability_service::refresh_all_live_backends(
                gs,
                crate::gateway::capability::RefreshReason::Periodic,
            )
            .await;
            match crate::gateway::capability_service::call_service(
                gs,
                slug,
                arguments,
                forwarded_meta,
                trace_context,
                agent_context,
            )
            .await
            {
                Ok(result) => {
                    record_search_followup(
                        gs,
                        search_id.as_deref(),
                        "call",
                        Some(slug),
                        None,
                        true,
                        trace_context,
                    );
                    (
                        serde_json::to_string_pretty(&result)
                            .unwrap_or_else(|_| result.to_string()),
                        false,
                    )
                }
                Err(err2) => {
                    record_search_followup(
                        gs,
                        search_id.as_deref(),
                        "call",
                        Some(slug),
                        None,
                        false,
                        trace_context,
                    );
                    let payload = crate::gateway::capability_service::service_error_to_json(&err2);
                    (
                        serde_json::to_string_pretty(&payload)
                            .unwrap_or_else(|_| err2.message.clone()),
                        true,
                    )
                }
            }
        }
        Err(err) => {
            record_search_followup(
                gs,
                search_id.as_deref(),
                "call",
                Some(slug),
                None,
                false,
                trace_context,
            );
            let payload = crate::gateway::capability_service::service_error_to_json(&err);
            (
                serde_json::to_string_pretty(&payload).unwrap_or_else(|_| err.message.clone()),
                true,
            )
        }
    }
}

/// Maximum number of backend invocations allowed in one `call_tools` /
/// `POST /v1/call_batch` request (token + backend fairness guardrail).
pub const MAX_CALL_TOOLS_BATCH: usize = 25;

/// Shared implementation for MCP `call_tools` and REST `POST /v1/call_batch`.
///
/// Request shape: `{ "calls": [ { "tool_slug", "arguments"?, "meta"? }, ... ],
/// "stop_on_error"?: bool }`. Each entry is routed through
/// [`crate::gateway::capability_service::call_service`] with the same
/// unknown-slug refresh-and-retry semantics as [`tool_call_tool`].
///
/// Returns `Ok(Value)` with `{ "success": bool, "results": [...] }` where each
/// result item includes `index`, optional client-supplied `id`, `tool_slug`,
/// `ok`, and either `result` or `error` (structured service error JSON).
/// Returns `Err(message)` for bad request shapes (missing `calls`, empty
/// array, over limit).
///
/// `mcp_meta` is optional MCP `_meta` from the outer `tools/call` envelope,
/// applied to each batch item when that item does not supply its own `meta`.
pub async fn gateway_call_batch_inner(
    gs: &GatewayState,
    args: &Value,
    mcp_meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    agent_context: Option<&AgentContext>,
) -> Result<Value, String> {
    let calls = args
        .get("calls")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing required field: calls (non-empty array)".to_string())?;
    if calls.is_empty() {
        return Err("calls must be a non-empty array".to_string());
    }
    if calls.len() > MAX_CALL_TOOLS_BATCH {
        return Err(format!(
            "calls exceeds maximum batch size ({MAX_CALL_TOOLS_BATCH})"
        ));
    }
    let stop_on_error = args
        .get("stop_on_error")
        .and_then(Value::as_bool)
        .unwrap_or(false);

    let mut results: Vec<Value> = Vec::with_capacity(calls.len());
    let mut all_ok = true;

    for (idx, call) in calls.iter().enumerate() {
        let item_id = call.get("id").cloned();
        let Some(slug) = call.get("tool_slug").and_then(Value::as_str) else {
            all_ok = false;
            let mut item = json!({
                "index": idx,
                "ok": false,
                "error": {"kind": "bad-request", "message": "missing tool_slug on call item"},
            });
            if let Some(id) = item_id {
                item["id"] = id;
            }
            results.push(item);
            if stop_on_error {
                break;
            }
            continue;
        };
        let arguments = match coerce_tool_arguments_object(call.get("arguments").cloned()) {
            Ok(v) => v,
            Err(msg) => return Err(msg),
        };
        let forwarded_meta = call.get("meta").cloned().or_else(|| mcp_meta.cloned());
        let search_id = call
            .get("meta")
            .and_then(search_id_from_meta)
            .or_else(|| mcp_meta.and_then(search_id_from_meta));
        let child_trace_context =
            trace_context.map(|ctx| ctx.child_request(format!("{}:batch-{idx}", ctx.request_id)));
        let child_trace_context = child_trace_context.as_ref().or(trace_context);

        let single_outcome = async {
            match crate::gateway::capability_service::call_service(
                gs,
                slug,
                arguments.clone(),
                forwarded_meta.clone(),
                child_trace_context,
                agent_context,
            )
            .await
            {
                Ok(result) => Ok(result),
                Err(err) if err.kind == "unknown-slug" => {
                    crate::gateway::capability_service::refresh_all_live_backends(
                        gs,
                        crate::gateway::capability::RefreshReason::Periodic,
                    )
                    .await;
                    crate::gateway::capability_service::call_service(
                        gs,
                        slug,
                        arguments,
                        forwarded_meta,
                        child_trace_context,
                        agent_context,
                    )
                    .await
                }
                Err(err) => Err(err),
            }
        }
        .await;

        match single_outcome {
            Ok(result) => {
                record_search_followup(
                    gs,
                    search_id.as_deref(),
                    "call",
                    Some(slug),
                    None,
                    true,
                    trace_context,
                );
                let mut item = json!({
                    "index": idx,
                    "tool_slug": slug,
                    "ok": true,
                    "result": result,
                });
                if let Some(id) = item_id {
                    item["id"] = id;
                }
                results.push(item);
            }
            Err(err) => {
                record_search_followup(
                    gs,
                    search_id.as_deref(),
                    "call",
                    Some(slug),
                    None,
                    false,
                    trace_context,
                );
                all_ok = false;
                let payload = crate::gateway::capability_service::service_error_to_json(&err);
                let mut item = json!({
                    "index": idx,
                    "tool_slug": slug,
                    "ok": false,
                    "error": payload,
                });
                if let Some(id) = item_id {
                    item["id"] = id;
                }
                results.push(item);
                if stop_on_error {
                    break;
                }
            }
        }
    }

    Ok(json!({
        "success": all_ok,
        "stop_on_error": stop_on_error,
        "results": results,
    }))
}

/// `call_tools` — invoke multiple backend capabilities in one MCP round-trip.
pub async fn tool_call_tools(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    agent_context: Option<&AgentContext>,
) -> (String, bool) {
    match gateway_call_batch_inner(gs, args, meta, trace_context, agent_context).await {
        Ok(value) => {
            let is_error = !value
                .get("success")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            (
                serde_json::to_string_pretty(&value).unwrap_or_else(|_| value.to_string()),
                is_error,
            )
        }
        Err(msg) => (msg, true),
    }
}

// ── private helpers ────────────────────────────────────────────────────────

pub(crate) fn record_load_skill_search_followup(
    gs: &GatewayState,
    args: &Value,
    meta: Option<&Value>,
    trace_context: Option<&TraceContext>,
    success: bool,
) {
    record_search_followup(
        gs,
        search_id_from_inputs(args, meta).as_deref(),
        "load_skill",
        None,
        skill_name_from_payload(args),
        success,
        trace_context,
    );
}

fn annotate_skill_search_payload(
    gs: &GatewayState,
    args: &Value,
    text: &str,
    trace_context: Option<&TraceContext>,
    session_id: Option<&str>,
    agent_context: Option<&AgentContext>,
) -> String {
    let search_id = crate::gateway::search_telemetry::SearchTelemetryStore::new_search_id();
    let index_generation =
        crate::gateway::capability_service::index_generation(&gs.capability_index);
    let mut payload = serde_json::from_str::<Value>(text).unwrap_or_else(|_| json!({"raw": text}));
    let mut telemetry_hits = Vec::new();
    let skills = payload
        .get_mut("skills")
        .and_then(Value::as_array_mut)
        .map(|items| {
            for (idx, skill) in items.iter_mut().enumerate() {
                if let Some(obj) = skill.as_object_mut() {
                    let rank = (idx + 1) as u32;
                    obj.entry("rank".to_string()).or_insert_with(|| json!(rank));
                    let skill_name = obj
                        .get("name")
                        .or_else(|| obj.get("skill_name"))
                        .or_else(|| obj.get("skill"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let tool_slug = obj
                        .get("tool_slug")
                        .or_else(|| obj.get("slug"))
                        .and_then(Value::as_str)
                        .unwrap_or(skill_name.as_str())
                        .to_string();
                    let dcc_type = obj
                        .get("_dcc_type")
                        .or_else(|| obj.get("dcc_type"))
                        .or_else(|| obj.get("dcc"))
                        .and_then(Value::as_str)
                        .unwrap_or_default()
                        .to_string();
                    let instance_id = obj
                        .get("_instance_id")
                        .or_else(|| obj.get("instance_id"))
                        .and_then(Value::as_str)
                        .map(str::to_string);
                    telemetry_hits.push(SearchTelemetryHit {
                        tool_slug,
                        skill_name: (!skill_name.is_empty()).then_some(skill_name.clone()),
                        dcc_type: dcc_type.clone(),
                        rank,
                        score: obj
                            .get("score")
                            .and_then(Value::as_u64)
                            .map_or(0, |score| score as u32),
                        match_reasons: obj
                            .get("match_reasons")
                            .and_then(Value::as_array)
                            .map(|items| {
                                items
                                    .iter()
                                    .filter_map(Value::as_str)
                                    .map(str::to_string)
                                    .collect()
                            })
                            .unwrap_or_default(),
                        loaded: obj.get("loaded").and_then(Value::as_bool).unwrap_or(false),
                    });
                    let mut next_args = json!({
                        "skill_name": skill_name,
                    });
                    if !dcc_type.is_empty() {
                        next_args["dcc"] = json!(dcc_type);
                    }
                    if let Some(instance_id) = instance_id {
                        next_args["instance_id"] = json!(instance_id);
                    }
                    attach_search_meta(&mut next_args, &search_id, &index_generation);
                    obj.insert(
                        "next_step".to_string(),
                        json!({
                            "action": "load_skill",
                            "arguments": next_args.clone(),
                            "mcp": {
                                "tool": "load_skill",
                                "arguments": next_args.clone(),
                                "_meta": next_args["meta"].clone(),
                            },
                            "rest": {
                                "method": "POST",
                                "path": "/v1/load_skill",
                                "body": next_args,
                            },
                        }),
                    );
                }
            }
            items.len()
        })
        .unwrap_or(0);
    if let Some(obj) = payload.as_object_mut() {
        obj.insert("search_id".to_string(), json!(search_id.clone()));
        obj.insert("ranker_version".to_string(), json!(RANKER_VERSION));
        obj.insert(
            "index_generation".to_string(),
            json!(index_generation.clone()),
        );
    }
    gs.search_telemetry.record_search(SearchTelemetryInput {
        search_id,
        transport: "mcp".to_string(),
        kind: "skill".to_string(),
        query: args
            .get("query")
            .and_then(Value::as_str)
            .unwrap_or_default()
            .to_string(),
        dcc_type: args
            .get("dcc_type")
            .or_else(|| args.get("dcc"))
            .and_then(Value::as_str)
            .map(str::to_string),
        instance_id: args
            .get("instance_id")
            .and_then(Value::as_str)
            .map(str::to_string),
        limit: args
            .get("limit")
            .and_then(Value::as_u64)
            .map(|value| value as u32),
        total: skills,
        ranker_version: RANKER_VERSION.to_string(),
        index_generation,
        hits: telemetry_hits,
        trace_context: trace_context.cloned(),
        session_id: session_id
            .map(str::to_string)
            .or_else(|| agent_context.and_then(|ctx| ctx.session_id.clone())),
        agent_context: agent_context.cloned(),
    });
    serde_json::to_string_pretty(&payload).unwrap_or_else(|_| text.to_string())
}

fn search_hits_for_telemetry(
    hits: &[crate::gateway::capability::SearchHit],
) -> Vec<SearchTelemetryHit> {
    hits.iter()
        .map(|hit| SearchTelemetryHit {
            tool_slug: hit.record.tool_slug.clone(),
            skill_name: hit.record.skill_name.clone(),
            dcc_type: hit.record.dcc_type.clone(),
            rank: hit.rank,
            score: hit.score,
            match_reasons: hit.match_reasons.clone(),
            loaded: hit.record.loaded,
        })
        .collect()
}

fn record_search_followup(
    gs: &GatewayState,
    search_id: Option<&str>,
    kind: &str,
    tool_slug: Option<&str>,
    skill_name: Option<String>,
    success: bool,
    trace_context: Option<&TraceContext>,
) {
    let Some(search_id) = search_id else {
        return;
    };
    gs.search_telemetry.record_followup(SearchFollowupInput {
        search_id: search_id.to_string(),
        kind: kind.to_string(),
        tool_slug: tool_slug.map(str::to_string),
        skill_name,
        success,
        trace_context: trace_context.cloned(),
    });
}

fn search_id_from_inputs(args: &Value, meta: Option<&Value>) -> Option<String> {
    search_id_from_payload(args).or_else(|| meta.and_then(search_id_from_meta))
}

fn index_generation_from_inputs(args: &Value, meta: Option<&Value>) -> Option<String> {
    fn from_payload(value: &Value) -> Option<String> {
        value
            .get("index_generation")
            .and_then(Value::as_str)
            .map(str::to_string)
            .or_else(|| {
                value
                    .get("meta")
                    .and_then(|meta| meta.get("index_generation"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
            .or_else(|| {
                value
                    .get("_meta")
                    .and_then(|meta| meta.get("index_generation"))
                    .and_then(Value::as_str)
                    .map(str::to_string)
            })
    }

    from_payload(args).or_else(|| meta.and_then(from_payload))
}

fn describe_needs_refresh(
    gs: &GatewayState,
    slug: &str,
    args: &Value,
    meta: Option<&Value>,
) -> bool {
    if let Some(generation) = index_generation_from_inputs(args, meta) {
        let current = crate::gateway::capability_service::index_generation(&gs.capability_index);
        if generation != current {
            return true;
        }
    }

    crate::gateway::capability_service::describe_service(&gs.capability_index, slug)
        .map(|_| false)
        .unwrap_or(true)
}

fn skill_name_from_payload(payload: &Value) -> Option<String> {
    payload
        .get("skill_name")
        .and_then(Value::as_str)
        .map(str::to_string)
        .or_else(|| {
            payload
                .get("skill_names")
                .and_then(Value::as_array)
                .and_then(|items| items.iter().find_map(Value::as_str))
                .map(str::to_string)
        })
}

fn call_next_step(slug: &str, search_id: &str) -> Value {
    let mut arguments = json!({
        "tool_slug": slug,
        "arguments": {},
    });
    attach_search_meta(&mut arguments, search_id, "");
    json!({
        "action": "call",
        "arguments": arguments.clone(),
        "mcp": {
            "tool": "call",
            "arguments": arguments.clone(),
            "_meta": arguments["meta"].clone(),
        },
        "rest": {
            "method": "POST",
            "path": "/v1/call",
            "body": arguments,
        },
    })
}

fn attach_search_meta(arguments: &mut Value, search_id: &str, index_generation: &str) {
    if let Some(obj) = arguments.as_object_mut() {
        let mut meta = json!({
            "search_id": search_id,
            "ranker_version": RANKER_VERSION,
        });
        if !index_generation.is_empty() {
            meta["index_generation"] = json!(index_generation);
        }
        obj.insert("meta".to_string(), meta);
    }
}

/// Return the advertised gateway MCP workflow surface.
///
/// The gateway intentionally advertises only four canonical workflow tools.
/// Backend per-action tools are discovered by `search` / `describe` and
/// invoked by `call`; older wrapper names remain callable as hidden
/// compatibility routes but do not consume model context in `tools/list`.
pub fn gateway_tool_defs() -> serde_json::Value {
    json!([
        {
            "name": "search",
            "description": "Discover backend capabilities and/or skills. Default `kind=tool` runs the \
                capability index (`search_tools` semantics): compact hits with `tool_slug` and \
                executable `next_step`. Follow `next_step`: tools with `has_schema=false` can go \
                straight to `call`; tools with schema still use `describe` first to fetch \
                `input_schema` / required parameter names. \
                `kind=skill` lists or searches skills (`list_skills` / `search_skills`). `kind=all` returns both.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "kind": {"type": "string", "enum": ["tool", "skill", "all"], "default": "tool"},
                    "query": {"type": "string"},
                    "dcc_type": {"type": "string"},
                    "dcc": {"type": "string", "description": "Alias of dcc_type for skill search"},
                    "tags": {"type": "array", "items": {"type": "string"}},
                    "limit": {"type": "integer", "minimum": 0},
                    "response_format": {"type": "string", "enum": ["json", "toon"], "description": "Wrapper-level output format. Prefer MCP params._meta.response_format for clients that keep tool arguments pure."},
                    "compact": {"type": "boolean", "description": "Alias for response_format=toon when true."}
                }
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": true}
        },
        {
            "name": "describe",
            "description": "Fetch full metadata. Pass `tool_slug` from `search` to get `input_schema`, \
                `properties`, and `required` (e.g. maya_geometry export uses `path`, not `destination`). \
                MCP describe refreshes backend capabilities only when the slug is missing or the \
                supplied `meta.index_generation` is stale. Pass `skill_name` for skill-level detail \
                (tools list, dependencies).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_slug": {"type": "string"},
                    "skill_name": {"type": "string"},
                    "dcc": {"type": "string"},
                    "meta": {"type": "object", "additionalProperties": true, "description": "Correlation metadata from search/load_skill next_step, including index_generation."},
                    "response_format": {"type": "string", "enum": ["json", "toon"], "description": "Wrapper-level output format. Prefer MCP params._meta.response_format for clients that keep tool arguments pure."},
                    "compact": {"type": "boolean", "description": "Alias for response_format=toon when true."}
                }
            },
            "annotations": {"readOnlyHint": true, "openWorldHint": true}
        },
        {
            "name": "load_skill",
            "description": "Load a discovered skill on a target DCC instance, or activate/deactivate a \
                progressive tool group. Use `skill_name` from search results and pass `instance_id` or \
                `dcc`/`dcc_type` when more than one backend is live. By default the gateway \
                activates all declared groups; set `activate_groups=false` for lazy loading, \
                or pass `tool_group` to activate one group explicitly. When following a correlated \
                search `next_step`, keep `target_tool_slug` and `meta.search_id`; the response may \
                inline `compact_schema` and point directly to `call`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name": {"type": "string"},
                    "skill_names": {"type": "array", "items": {"type": "string"}},
                    "activate_groups": {"type": "boolean", "default": true},
                    "tool_group": {"type": "string", "description": "Progressive group to activate after loading."},
                    "group_name": {"type": "string", "description": "Alias of tool_group."},
                    "group_action": {"type": "string", "enum": ["activate", "deactivate"], "default": "activate"},
                    "instance_id": {"type": "string", "description": "Target instance UUID or unique prefix."},
                    "dcc": {"type": "string", "description": "DCC type filter such as maya, blender, or a custom host."},
                    "dcc_type": {"type": "string", "description": "Alias of dcc."},
                    "target_tool_slug": {"type": "string", "description": "Tool slug from a correlated search hit; lets load_skill inline compact_schema for the intended follow-up call."},
                    "meta": {"type": "object", "additionalProperties": true, "description": "Correlation metadata from search next_step, including search_id and index_generation."},
                    "response_format": {"type": "string", "enum": ["json", "toon"], "description": "Wrapper-level output format. Prefer MCP params._meta.response_format for clients that keep tool arguments pure."},
                    "compact": {"type": "boolean", "description": "Alias for response_format=toon when true."}
                },
                "required": ["skill_name"]
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": false,
                "idempotentHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "call",
            "description": "Invoke one backend capability by `tool_slug`, or run an ordered batch with \
                `calls` (maximum 25). Copy parameter names from `describe` or `load_skill.compact_schema` \
                into `arguments`; `has_schema=false` tools can use empty `{}` arguments. \
                backend-specific fields never belong at this wrapper's top level.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "tool_slug": {"type": "string"},
                    "arguments": {"type": "object", "additionalProperties": true, "default": {}},
                    "meta": {"type": "object", "additionalProperties": true},
                    "calls": {
                        "type": "array",
                        "maxItems": 25,
                        "items": {
                            "type": "object",
                            "properties": {
                                "tool_slug": {"type": "string"},
                                "arguments": {"type": "object", "additionalProperties": true, "default": {}},
                                "meta": {"type": "object", "additionalProperties": true}
                            },
                            "required": ["tool_slug"]
                        }
                    },
                    "stop_on_error": {"type": "boolean", "default": false},
                    "response_format": {"type": "string", "enum": ["json", "toon"], "description": "Wrapper-level output format; it is not forwarded to the backend capability."},
                    "compact": {"type": "boolean", "description": "Alias for response_format=toon when true."}
                }
            },
            "annotations": {
                "readOnlyHint": false,
                "destructiveHint": true,
                "idempotentHint": false,
                "openWorldHint": true
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value, json};
    use std::sync::Arc;
    use std::time::Duration;
    use tokio::sync::{RwLock, broadcast, watch};
    use uuid::Uuid;

    fn test_gateway_state() -> GatewayState {
        let dir = tempfile::tempdir().unwrap();
        let (yield_tx, _) = watch::channel(false);
        let (events_tx, _) = broadcast::channel::<String>(8);
        GatewayState {
            registry: Arc::new(RwLock::new(
                dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
            )),
            http_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::http_registration::HttpInstanceRegistry::default(),
            )),
            mdns_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::mdns_registration::MdnsInstanceRegistry::default(),
            )),
            relay_instance_registry: Arc::new(parking_lot::RwLock::new(
                crate::gateway::relay_registration::RelayInstanceRegistry::default(),
            )),
            stale_timeout: Duration::from_secs(30),
            backend_timeout: Duration::from_secs(10),
            async_dispatch_timeout: Duration::from_secs(60),
            wait_terminal_timeout: Duration::from_secs(600),
            server_name: "test".into(),
            server_version: env!("CARGO_PKG_VERSION").into(),
            own_host: "127.0.0.1".into(),
            own_port: 0,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(events_tx),
            protocol_version: Arc::new(RwLock::new(None)),
            resource_subscriptions: Arc::new(RwLock::new(std::collections::HashMap::new())),
            client_attribution: Arc::new(
                crate::gateway::caller_attribution::ClientAttributionStore::default(),
            ),
            pending_calls: Arc::new(RwLock::new(std::collections::HashMap::new())),
            subscriber: crate::gateway::sse_subscriber::SubscriberManager::default(),
            allow_unknown_tools: false,
            policy: Arc::new(crate::gateway::GatewayPolicy::default()),
            adapter_version: None,
            adapter_dcc: None,
            capability_index: Arc::new(crate::gateway::capability::CapabilityIndex::new()),
            event_log: Arc::new(crate::gateway::event_log::EventLog::new()),
            #[cfg(feature = "prometheus")]
            gateway_metrics: Arc::new(crate::gateway::event_log::GatewayMetrics::new()),
            middleware_chain: Arc::new(crate::gateway::middleware::MiddlewareChain::new()),
            instance_diagnostics: Arc::new(
                crate::gateway::instance_diagnostics::InstanceDiagnosticsStore::new(),
            ),
            traffic_capture: Arc::new(crate::gateway::traffic::TrafficCapture::disabled()),
            search_telemetry: Arc::new(
                crate::gateway::search_telemetry::SearchTelemetryStore::new(),
            ),
            debug_routes_enabled: false,
            auth: Arc::new(crate::gateway::security::GatewayAuth::disabled()),
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
        }
    }

    fn annotations_by_tool() -> Map<String, Value> {
        gateway_tool_defs()
            .as_array()
            .expect("gateway_tool_defs returns an array")
            .iter()
            .map(|tool| {
                let name = tool
                    .get("name")
                    .and_then(Value::as_str)
                    .expect("gateway tool has a name")
                    .to_string();
                let annotations = tool
                    .get("annotations")
                    .cloned()
                    .expect("gateway tool has annotations");
                (name, annotations)
            })
            .collect()
    }

    #[test]
    fn gateway_tool_defs_advertise_canonical_workflow_tools_only() {
        let defs = gateway_tool_defs();
        let names: Vec<&str> = defs
            .as_array()
            .expect("gateway_tool_defs returns an array")
            .iter()
            .filter_map(|tool| tool.get("name").and_then(Value::as_str))
            .collect();

        assert_eq!(names, ["search", "describe", "load_skill", "call"]);
    }

    #[test]
    fn gateway_tool_defs_all_have_annotations() {
        let annotations = annotations_by_tool();
        assert_eq!(annotations.len(), 4);

        for (name, value) in annotations {
            let hints = value
                .as_object()
                .unwrap_or_else(|| panic!("{name} annotations must be an object"));
            assert!(
                [
                    "readOnlyHint",
                    "destructiveHint",
                    "idempotentHint",
                    "openWorldHint"
                ]
                .iter()
                .any(|key| hints.contains_key(*key)),
                "{name} annotations must include at least one MCP ToolAnnotations hint"
            );
        }
    }

    #[test]
    fn gateway_call_schema_keeps_compatibility_shape() {
        let defs = gateway_tool_defs();
        let call = defs
            .as_array()
            .unwrap()
            .iter()
            .find(|tool| tool["name"] == "call")
            .expect("call tool advertised");

        assert_eq!(call["inputSchema"]["type"], "object");
        assert!(call["inputSchema"].get("anyOf").is_none());
        assert!(call["inputSchema"].get("oneOf").is_none());
        assert!(call["inputSchema"].get("allOf").is_none());
        assert!(call["inputSchema"].get("not").is_none());
        assert!(call["inputSchema"].get("required").is_none());
        assert!(call["inputSchema"]["properties"]["tool_slug"].is_object());
        assert_eq!(call["inputSchema"]["properties"]["calls"]["maxItems"], 25);
        assert_eq!(call["annotations"]["destructiveHint"], true);
    }

    #[test]
    fn gateway_tool_defs_use_expected_annotations() {
        let annotations = annotations_by_tool();

        assert_eq!(
            annotations.get("search"),
            Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
        );
        assert_eq!(
            annotations.get("describe"),
            Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
        );
    }

    #[test]
    fn describe_refresh_is_conditional_on_generation_and_index_hit() {
        let gs = test_gateway_state();
        let instance_id = Uuid::from_u128(0x1234);
        let record = crate::gateway::capability::CapabilityRecord::new(
            crate::gateway::capability::tool_slug("maya", &instance_id, "maya_scene__list_objects"),
            "maya_scene__list_objects".to_string(),
            "maya_scene__list_objects".to_string(),
            Some("maya-scene".into()),
            "List scene objects",
            vec!["scene".into()],
            "maya".into(),
            instance_id,
            false,
            true,
            None,
        );
        let fingerprint = crate::gateway::capability::InstanceFingerprint(1);
        gs.capability_index
            .upsert_instance(instance_id, vec![record.clone()], fingerprint);

        let current_generation =
            crate::gateway::capability_service::index_generation(&gs.capability_index);
        let args = json!({
            "tool_slug": record.tool_slug,
            "meta": {"index_generation": current_generation}
        });
        assert!(!describe_needs_refresh(&gs, &record.tool_slug, &args, None));

        let stale_args = json!({
            "tool_slug": record.tool_slug,
            "meta": {"index_generation": "stale"}
        });
        assert!(describe_needs_refresh(
            &gs,
            &record.tool_slug,
            &stale_args,
            None
        ));

        assert!(describe_needs_refresh(
            &gs,
            "maya.abcdef01.__missing__",
            &json!({}),
            None
        ));
    }
}
