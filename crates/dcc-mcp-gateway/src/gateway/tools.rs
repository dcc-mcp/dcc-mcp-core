//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};
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
        "instance": entry_to_json(&entry, gs.stale_timeout),
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
                "instance": entry_to_json(&entry, gs.stale_timeout),
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
        "instance": entry_to_json(&released, gs.stale_timeout),
    }))
    .map_err(|e| e.to_string())
}

// ── #655 dynamic-capability MCP wrappers ──────────────────────────────────

/// `search_tools` — MCP wrapper that routes to
/// [`crate::gateway::capability_service::search_service`].
///
/// Kept alongside the REST handler so both transports produce
/// byte-identical responses for the same query.
pub async fn tool_search_tools(gs: &GatewayState, args: &Value) -> Result<String, String> {
    // Refresh on demand so the first query after startup (or after
    // a skill load/unload) always sees current capabilities.
    crate::gateway::capability_service::refresh_all_live_backends(
        gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    let query = crate::gateway::capability_service::parse_search_payload(args);
    let hits = crate::gateway::capability_service::search_service(&gs.capability_index, &query);

    // Annotate unloaded hits with a structured `next_step` so agents
    // know they must call `load_skill` before invoking the tool.
    let mut annotated: Vec<Value> = hits
        .into_iter()
        .map(|h| {
            let mut v = serde_json::to_value(&h).unwrap_or(Value::Null);
            if !h.record.loaded
                && let Some(skill_name) = &h.record.skill_name
            {
                v["next_step"] = json!({
                    "action": "load_skill",
                    "skill_name": skill_name,
                });
            }
            v
        })
        .collect();
    annotated.extend(local_gateway_tool_hits(&query));

    serde_json::to_string_pretty(&json!({
        "total": annotated.len(),
        "hits":  annotated,
    }))
    .map_err(|e| e.to_string())
}

fn local_gateway_tool_hits(query: &crate::gateway::capability::SearchQuery) -> Vec<Value> {
    let mut clauses: Vec<String> = Vec::new();
    let q = query.query.trim().to_ascii_lowercase();
    if !q.is_empty() {
        clauses.push(q);
    }
    for o in &query.or_queries {
        let t = o.trim().to_ascii_lowercase();
        if !t.is_empty() && !clauses.contains(&t) {
            clauses.push(t);
        }
    }
    let exclude: Vec<String> = query
        .exclude_tags
        .iter()
        .map(|t| t.trim().to_ascii_lowercase())
        .filter(|t| !t.is_empty())
        .collect();

    let tools = [
        (
            "call_tools",
            "Invoke multiple backend DCC capabilities in one ordered batch (max 25); REST POST /v1/call_batch.",
            vec!["gateway", "batch", "dispatch"],
        ),
        (
            "activate_tool_group",
            "Activate a progressive tool group on a DCC instance after lazy loading.",
            vec!["gateway", "group", "skill-management"],
        ),
        (
            "deactivate_tool_group",
            "Deactivate a progressive tool group on a DCC instance.",
            vec!["gateway", "group", "skill-management"],
        ),
    ];

    tools
        .into_iter()
        .filter(|(_, _, tags)| {
            !exclude
                .iter()
                .any(|ex| tags.iter().any(|t| t.to_ascii_lowercase() == *ex))
        })
        .filter(|(name, summary, _)| {
            clauses.is_empty()
                || clauses.iter().any(|c| {
                    c.is_empty()
                        || name.contains(c)
                        || summary.to_ascii_lowercase().contains(c)
                        || c == "group"
                        || (c.contains("batch") && *name == "call_tools")
                })
        })
        .map(|(name, summary, tags)| {
            json!({
                "tool_slug": format!("gateway.{name}"),
                "backend_tool": name,
                "callable_id": name,
                "skill_name": null,
                "summary": summary,
                "tags": tags,
                "dcc_type": "gateway",
                "instance_id": uuid::Uuid::nil(),
                "has_schema": true,
                "loaded": true,
                "score": 100,
                "next_step": {
                    "action": "tools/call",
                    "name": name,
                },
            })
        })
        .collect()
}

/// `describe_tool` — MCP wrapper around
/// [`crate::gateway::capability_service::describe_service`].
pub async fn tool_describe_tool(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return Err("missing required argument: tool_slug".to_string());
    };
    // Refresh on demand — a `describe_tool` call immediately after
    // `load_skill` must see the newly-registered action.
    crate::gateway::capability_service::refresh_all_live_backends(
        gs,
        crate::gateway::capability::RefreshReason::Periodic,
    )
    .await;
    match crate::gateway::capability_service::describe_tool_full(gs, slug).await {
        Ok((record, tool)) => serde_json::to_string_pretty(&json!({
            "record": record,
            "tool":   tool,
        }))
        .map_err(|e| e.to_string()),
        Err(err) => {
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
) -> (String, bool) {
    let Some(slug) = args.get("tool_slug").and_then(|v| v.as_str()) else {
        return ("missing required argument: tool_slug".to_string(), true);
    };
    let arguments = args.get("arguments").cloned().unwrap_or_else(|| json!({}));
    let forwarded_meta = args.get("meta").cloned().or_else(|| meta.cloned());
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
    )
    .await
    {
        Ok(result) => (
            serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
            false,
        ),
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
            )
            .await
            {
                Ok(result) => (
                    serde_json::to_string_pretty(&result).unwrap_or_else(|_| result.to_string()),
                    false,
                ),
                Err(err2) => {
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
/// result item includes `index`, `tool_slug`, `ok`, and either `result` or
/// `error` (structured service error JSON). Returns `Err(message)` for bad
/// request shapes (missing `calls`, empty array, over limit).
///
/// `mcp_meta` is optional MCP `_meta` from the outer `tools/call` envelope,
/// applied to each batch item when that item does not supply its own `meta`.
pub async fn gateway_call_batch_inner(
    gs: &GatewayState,
    args: &Value,
    mcp_meta: Option<&Value>,
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
        let Some(slug) = call.get("tool_slug").and_then(Value::as_str) else {
            all_ok = false;
            results.push(json!({
                "index": idx,
                "ok": false,
                "error": {"kind": "bad-request", "message": "missing tool_slug on call item"},
            }));
            if stop_on_error {
                break;
            }
            continue;
        };
        let arguments = call.get("arguments").cloned().unwrap_or_else(|| json!({}));
        let forwarded_meta = call.get("meta").cloned().or_else(|| mcp_meta.cloned());

        let single_outcome = async {
            match crate::gateway::capability_service::call_service(
                gs,
                slug,
                arguments.clone(),
                forwarded_meta.clone(),
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
                    )
                    .await
                }
                Err(err) => Err(err),
            }
        }
        .await;

        match single_outcome {
            Ok(result) => {
                results.push(json!({
                    "index": idx,
                    "tool_slug": slug,
                    "ok": true,
                    "result": result,
                }));
            }
            Err(err) => {
                all_ok = false;
                let payload = crate::gateway::capability_service::service_error_to_json(&err);
                results.push(json!({
                    "index": idx,
                    "tool_slug": slug,
                    "ok": false,
                    "error": payload,
                }));
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
) -> (String, bool) {
    let _ = meta;
    match gateway_call_batch_inner(gs, args, meta).await {
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

/// Return the JSON schema for gateway discovery, pooling, and dynamic-capability tools.
pub fn gateway_tool_defs() -> serde_json::Value {
    json!([
        {
            "name": "acquire_dcc_instance",
            "description": "Reserve an idle DCC instance for a workflow or long-running job. \
                This marks the instance busy and stores lease metadata in the shared registry. \
                Pooling is optional; simple single-instance adapters can ignore this tool.",
            "inputSchema": {
                "type": "object",
                "required": ["dcc_type"],
                "properties": {
                    "dcc_type":       {"type": "string", "description": "DCC type to lease (e.g. 'maya')"},
                    "instance_id":    {"type": "string", "description": "Optional UUID or unique prefix to lease a specific instance"},
                    "lease_owner":    {"type": "string", "description": "Client/workflow owner label for the lease"},
                    "current_job_id": {"type": "string", "description": "Optional job id associated with the lease"},
                    "ttl_secs":       {"type": "integer", "minimum": 1, "description": "Lease TTL in seconds (default: 3600)"}
                }
            },
            "annotations": {
                "destructiveHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "release_dcc_instance",
            "description": "Clear a pool lease previously created by acquire_dcc_instance in the shared FileRegistry (services.json). \
                Does not terminate the DCC process or disconnect MCP clients — it only flips registry metadata so another workflow can acquire the slot. \
                Must match the same lease_owner string passed to acquire when that optional argument is used.",
            "inputSchema": {
                "type": "object",
                "required": ["instance_id"],
                "properties": {
                    "instance_id": {"type": "string", "description": "UUID or unique prefix from list_dcc_instances"},
                    "lease_owner": {"type": "string", "description": "Optional owner guard; when provided it must match the active lease owner"}
                }
            },
            "annotations": {
                "destructiveHint": false,
                "openWorldHint": true
            }
        },
        {
            "name": "search_tools",
            "description": "Search dynamic DCC capabilities by keyword, DCC type, tags, or scene hint. \
                Returns compact records (not full schemas) so token cost stays bounded even when \
                many DCC instances are live. Each hit includes `tool_slug` — copy that string verbatim \
                into `describe_tool` and `call_tool`; do not substitute ad-hoc ids or script-style \
                top-level fields. Use `describe_tool` to fetch one capability's schema, then `call_tool` \
                or `call_tools` to invoke. REST twins: `POST /v1/search`, `/v1/describe`, `/v1/call` (#657).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":      {"type": "string", "description": "Keyword(s) matched against tool name, summary, tags, and skill name."},
                    "dcc_type":   {"type": "string", "description": "Optional DCC bucket filter (e.g. 'maya', 'blender')."},
                    "tags":       {"type": "array", "items": {"type": "string"}, "description": "Require every tag to be present."},
                    "exclude_tags": {"type": "array", "items": {"type": "string"}, "description": "Drop capabilities that carry any of these tags (case-insensitive exact match)."},
                    "scene_hint": {"type": "string", "description": "Optional scene/document hint used as a soft boost."},
                    "skill_hint": {"type": "string", "description": "Soft score bonus when the backing skill name contains this substring."},
                    "or_queries": {"type": "array", "items": {"type": "string"}, "description": "OR search branches: score is the max across `query` and each non-empty string here."},
                    "min_score":  {"type": "integer", "minimum": 0, "description": "When any search clause is present, drop hits below this final score (browse mode ignores this)."},
                    "limit":      {"type": "integer", "minimum": 0, "description": "Page size cap (default 25, max 100)."}
                }
            },
            "annotations": {
                "readOnlyHint": true,
                "openWorldHint": true
            }
        },
        {
            "name": "describe_tool",
            "description": "Resolve a single capability slug returned by `search_tools` back to its \
                compact record (name, skill, summary, tags, whether it has a schema, and the \
                backing instance id). Use this before `call_tool` when the caller needs the \
                action's metadata without invoking it. The `tool_slug` argument must match a hit \
                from `search_tools` exactly (same string as the REST `/v1/describe` body field).",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {
                        "type": "string",
                        "description": "Capability id from `search_tools`: `<dcc_type>.<instance_prefix_or_uuid>.<backend_tool>` \
                            (e.g. `maya.277685a7.maya_primitives__create_sphere`). Copy verbatim; it encodes \
                            DCC bucket, instance, and backend tool name for gateway routing — analogous to a \
                            REST path `.../<dcc>/<instance>/<backend_tool>` even though the wire format uses dots."
                    }
                }
            },
            "annotations": {
                "readOnlyHint": true,
                "openWorldHint": true
            }
        },
        {
            "name": "call_tool",
            "description": "Invoke one backend DCC action identified by `tool_slug`. REQUIRED: set \
                `tool_slug` to the exact string from `search_tools` / `describe_tool` (never omit it). \
                Put **all** tool-specific parameters inside the `arguments` object per that tool's schema — \
                this wrapper is **not** `execute_python` / arbitrary script execution: do **not** pass \
                `code`, `python`, `mel`, `script`, or similar keys at the **top** level of this payload. \
                Routes like REST `POST /v1/call` with the same `tool_slug` + `arguments` shape. Progress \
                notifications, cancellation, and async job routing match per-backend MCP behaviour.",
            "inputSchema": {
                "type": "object",
                "additionalProperties": false,
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {
                        "type": "string",
                        "description": "Exact capability id from `search_tools` (same as `POST /v1/call` `tool_slug`)."
                    },
                    "arguments": {
                        "type": "object",
                        "description": "JSON object forwarded to the backend tool's `tools/call` arguments (may include `code` **only** when that specific backend tool's schema requires it)."
                    },
                    "meta": {
                        "type": "object",
                        "description": "Optional MCP `_meta` passthrough (e.g. `dcc.async`)."
                    }
                }
            },
            "annotations": {
                "destructiveHint": true,
                "openWorldHint": true,
                "idempotentHint": false
            }
        },
        {
            "name": "call_tools",
            "description": "Invoke multiple DCC actions in order within one MCP request. Each element \
                of `calls` uses the same shape as `call_tool` (`tool_slug`, optional `arguments`, \
                optional per-item `meta`); never put `code`/`script` at the call-item top level — only \
                inside `arguments` when the backend schema says so. Optional `stop_on_error` (default false) \
                aborts the remainder after the first failed item. Maximum batch size is 25. REST twin: \
                `POST /v1/call_batch`.",
            "inputSchema": {
                "type": "object",
                "required": ["calls"],
                "properties": {
                    "calls": {
                        "type": "array",
                        "minItems": 1,
                        "maxItems": 25,
                        "items": {
                            "type": "object",
                            "additionalProperties": false,
                            "required": ["tool_slug"],
                            "properties": {
                                "tool_slug": {
                                    "type": "string",
                                    "description": "Same `tool_slug` rules as `call_tool`."
                                },
                                "arguments": {"type": "object"},
                                "meta": {"type": "object"}
                            }
                        },
                        "description": "Ordered backend invocations (same routing as call_tool)."
                    },
                    "stop_on_error": {
                        "type": "boolean",
                        "default": false,
                        "description": "When true, stop after the first failed invocation."
                    }
                }
            },
            "annotations": {
                "destructiveHint": true,
                "openWorldHint": true,
                "idempotentHint": false
            }
        }
    ])
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::{Map, Value, json};

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
    fn local_gateway_tool_hits_find_group_management_tools() {
        let q = crate::gateway::capability::SearchQuery {
            query: "group".into(),
            ..Default::default()
        };
        let hits = local_gateway_tool_hits(&q);
        let names: Vec<&str> = hits
            .iter()
            .filter_map(|hit| hit.get("backend_tool").and_then(Value::as_str))
            .collect();

        assert!(names.contains(&"activate_tool_group"));
        assert!(names.contains(&"deactivate_tool_group"));
    }

    #[test]
    fn gateway_tool_defs_all_have_annotations() {
        let annotations = annotations_by_tool();
        assert_eq!(annotations.len(), 6);

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
    fn gateway_tool_defs_use_expected_annotations() {
        let annotations = annotations_by_tool();

        assert_eq!(
            annotations.get("acquire_dcc_instance"),
            Some(&json!({"destructiveHint": false, "openWorldHint": true}))
        );
        assert_eq!(
            annotations.get("release_dcc_instance"),
            Some(&json!({"destructiveHint": false, "openWorldHint": true}))
        );
        assert_eq!(
            annotations.get("search_tools"),
            Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
        );
        assert_eq!(
            annotations.get("describe_tool"),
            Some(&json!({"readOnlyHint": true, "openWorldHint": true}))
        );
        assert_eq!(
            annotations.get("call_tool"),
            Some(&json!({
                "destructiveHint": true,
                "openWorldHint": true,
                "idempotentHint": false,
            }))
        );
        assert_eq!(
            annotations.get("call_tools"),
            Some(&json!({
                "destructiveHint": true,
                "openWorldHint": true,
                "idempotentHint": false,
            }))
        );
    }
}
