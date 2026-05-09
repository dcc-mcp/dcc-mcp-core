//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey};

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
    let Some(entry) = reg
        .acquire_lease(
            dcc_type,
            instance_id,
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
    let all = gs.all_instances(&reg);
    let matches: Vec<&ServiceEntry> = all
        .iter()
        .filter(|e| {
            let full = e.instance_id.to_string();
            full == instance_id || full.starts_with(instance_id)
        })
        .collect();
    let entry = match matches.as_slice() {
        [] => return Err(format!("Instance '{instance_id}' not found")),
        [entry] => *entry,
        _ => return Err(format!("Instance prefix '{instance_id}' is ambiguous")),
    };
    let key = ServiceKey {
        dcc_type: entry.dcc_type.clone(),
        instance_id: entry.instance_id,
    };
    let Some(released) = reg.release_lease(&key, owner).map_err(|e| e.to_string())? else {
        return Err("No matching active lease to release".to_string());
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
    serde_json::to_string_pretty(&json!({
        "total": hits.len(),
        "hits":  hits,
    }))
    .map_err(|e| e.to_string())
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
            "description": "Release a DCC instance lease acquired with acquire_dcc_instance.",
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
                many DCC instances are live. Use `describe_tool` to fetch one capability's schema, \
                then `call_tool` to invoke it. Part of the REST-backed dynamic-capability API (#657).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query":      {"type": "string", "description": "Keyword(s) matched against tool name, summary, tags, and skill name."},
                    "dcc_type":   {"type": "string", "description": "Optional DCC bucket filter (e.g. 'maya', 'blender')."},
                    "tags":       {"type": "array", "items": {"type": "string"}, "description": "Require every tag to be present."},
                    "scene_hint": {"type": "string", "description": "Optional scene/document hint used as a soft boost."},
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
                action's metadata without invoking it.",
            "inputSchema": {
                "type": "object",
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {"type": "string", "description": "Capability slug in the form `<dcc>.<id8>.<tool>`."}
                }
            },
            "annotations": {
                "readOnlyHint": true,
                "openWorldHint": true
            }
        },
        {
            "name": "call_tool",
            "description": "Invoke a DCC action by capability slug. Routes through the same \
                backend-forwarding machinery as the legacy prefixed gateway tools, so progress \
                notifications, cancellation, and job routing all work identically. Arguments are \
                the backend tool's usual JSON payload; `meta` is optional MCP `_meta` passthrough.",
            "inputSchema": {
                "type": "object",
                "required": ["tool_slug"],
                "properties": {
                    "tool_slug": {"type": "string", "description": "Capability slug from `search_tools` / `describe_tool`."},
                    "arguments": {"type": "object", "description": "Tool arguments forwarded to the backend."},
                    "meta":      {"type": "object", "description": "Optional MCP `_meta` passthrough (e.g. `dcc.async`)."}
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
    fn gateway_tool_defs_all_have_annotations() {
        let annotations = annotations_by_tool();
        assert_eq!(annotations.len(), 5);

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
    }
}
