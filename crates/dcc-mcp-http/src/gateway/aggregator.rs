//! Tools-list aggregation and tools-call routing for the facade gateway.
//!
//! This module is the core of the "one endpoint, every DCC" façade:
//!
//! * `aggregate_tools_list` — fan out `tools/list` to every live backend and
//!   merge the results.  Backend-provided tools get an instance prefix so
//!   identical tool names across multiple DCCs never clash (see [`namespace`]).
//! * `route_tools_call` — dispatch a `tools/call` based on the tool name:
//!   - Meta / skill-management tools are handled locally or fanned-out with
//!     instance-scoped semantics.
//!   - Prefixed tools are forwarded to the backend that owns them.
//!
//! All network I/O goes through the stateless helpers in
//! [`super::backend_client`], so fan-out works concurrently via `join_all`.

use std::time::Duration;

use futures::future::join_all;
use serde_json::{Value, json};
use uuid::Uuid;

use super::backend_client::{fetch_tools, forward_tools_call};
use super::namespace::{decode_tool_name, encode_tool_name, instance_short, is_local_tool};
use super::state::GatewayState;
use super::tools::{
    gateway_tool_defs, tool_connect_to_dcc, tool_get_instance, tool_list_instances,
};
use crate::protocol::{TOOLS_LIST_PAGE_SIZE, decode_cursor, encode_cursor};
use dcc_mcp_transport::discovery::types::ServiceEntry;

/// Build the unified `tools/list` result by aggregating every live backend.
///
/// Tool order:
/// 1. Gateway discovery meta-tools (`list_dcc_instances`, `get_dcc_instance`, `connect_to_dcc`).
/// 2. Skill-management tools (one canonical set for the whole gateway).
/// 3. Backend-provided tools from every live instance, prefixed with the
///    8-char instance id, annotated with `_instance_id` / `_dcc_type` in the
///    tool's `annotations` map so agents can display origin context.
///
/// Pagination uses the same cursor scheme as the per-DCC server:
/// `cursor` is an opaque hex-encoded offset into the flat tool list.
pub async fn aggregate_tools_list(gs: &GatewayState, cursor: Option<&str>) -> Value {
    let mut tools: Vec<Value> = Vec::new();

    // Tier 1 + 2: local gateway tools (meta + skill management).
    if let Value::Array(local) = gateway_tool_defs() {
        tools.extend(local);
    }
    tools.extend(skill_management_tool_defs());

    // Tier 3: fan out to every live backend.
    let instances = live_backends(gs).await;
    let client = &gs.http_client;
    let backend_timeout = gs.backend_timeout;
    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let backend_tools = fetch_tools(client, &url, backend_timeout).await;
        (entry.instance_id, entry.dcc_type.clone(), backend_tools)
    });
    let results = join_all(futs).await;

    for (iid, dcc_type, backend_tools) in results {
        for mut tool in backend_tools {
            // Skip any tool whose name would collide with a gateway-local name
            // AFTER encoding — cannot happen today because local tools are
            // already filtered by `is_local_tool`, but guard defensively.
            if is_local_tool(&tool.name) {
                continue;
            }
            let encoded = encode_tool_name(&iid, &tool.name);
            tool.name = encoded;
            let mut json_val = serde_json::to_value(&tool).unwrap_or(Value::Null);
            inject_instance_metadata(&mut json_val, &iid, &dcc_type);
            tools.push(json_val);
        }
    }

    // ── Pagination ───────────────────────────────────────────────────────
    let offset = cursor.and_then(decode_cursor).unwrap_or(0);
    let total = tools.len();
    let page_end = (offset + TOOLS_LIST_PAGE_SIZE).min(total);
    let page: Vec<Value> = if offset < total {
        tools.drain(offset..page_end).collect()
    } else {
        Vec::new()
    };

    let mut result = json!({"tools": page});
    if page_end < total {
        result["nextCursor"] = json!(encode_cursor(page_end));
    }
    result
}

/// Dispatch a gateway `tools/call` to the right place.
///
/// Returns `(text_body, is_error)` so the caller can wrap into an MCP
/// `CallToolResult`.  Agents never see backend URLs; results look identical
/// to those produced by a single-DCC server.
pub async fn route_tools_call(
    gs: &GatewayState,
    tool: &str,
    args: &Value,
    meta: Option<&Value>,
    request_id: Option<String>,
) -> (String, bool) {
    // ── Local meta-tools ────────────────────────────────────────────────
    match tool {
        "list_dcc_instances" => return to_text_result(tool_list_instances(gs, args).await),
        "get_dcc_instance" => return to_text_result(tool_get_instance(gs, args).await),
        "connect_to_dcc" => return to_text_result(tool_connect_to_dcc(gs, args).await),
        _ => {}
    }

    // ── Skill-management tools ──────────────────────────────────────────
    if matches!(
        tool,
        "list_skills"
            | "find_skills"
            | "search_skills"
            | "get_skill_info"
            | "load_skill"
            | "unload_skill"
    ) {
        return skill_mgmt_dispatch(gs, tool, args).await;
    }

    // ── Prefixed backend tool ───────────────────────────────────────────
    let Some((prefix, original)) = decode_tool_name(tool) else {
        return (
            format!(
                "Unknown tool: {tool}. Call list_dcc_instances or search_skills to discover tools."
            ),
            true,
        );
    };

    let Some(entry) = find_instance_by_prefix(gs, prefix).await else {
        return (
            format!("No live DCC instance matches prefix '{prefix}' in tool '{tool}'."),
            true,
        );
    };

    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
    // Update the pending-calls map with the real backend URL so that a
    // later notifications/cancelled can be forwarded to the right server.
    if let Some(ref rid) = request_id {
        let mut pending = gs.pending_calls.write().await;
        if let Some(call) = pending.get_mut(rid) {
            call.backend_url = url.clone();
        }
    }
    match forward_tools_call(
        &gs.http_client,
        &url,
        original,
        Some(args.clone()),
        meta.cloned(),
        request_id,
        gs.backend_timeout,
    )
    .await
    {
        Ok(mut result) => {
            // Backend already returns a CallToolResult { content, isError }.
            // Extract the actual text payload so the gateway's own response
            // is a single CallToolResult rather than a nested envelope.
            inject_instance_metadata(&mut result, &entry.instance_id, &entry.dcc_type);
            let is_error = result
                .get("isError")
                .and_then(Value::as_bool)
                .unwrap_or(false);
            let text = result
                .get("content")
                .and_then(Value::as_array)
                .and_then(|arr| arr.first())
                .and_then(|c| c.get("text"))
                .and_then(Value::as_str)
                .map(str::to_owned)
                .unwrap_or_else(|| serde_json::to_string_pretty(&result).unwrap_or_default());
            (text, is_error)
        }
        Err(e) => (format!("Backend call failed: {e}"), true),
    }
}

// ── Skill-management dispatch ──────────────────────────────────────────────

/// Dispatch a skill-management tool across backends.
///
/// Two patterns:
/// * Fan-out, aggregate (`list_skills`, `find_skills`, `search_skills`,
///   `get_skill_info`): call every matching backend, merge results with
///   `_instance_id` / `_dcc_type` annotations so agents can disambiguate.
/// * Target-instance (`load_skill`, `unload_skill`): require `instance_id` /
///   `dcc` in the arguments; if a single backend is live these default
///   automatically.
async fn skill_mgmt_dispatch(gs: &GatewayState, tool: &str, args: &Value) -> (String, bool) {
    let dcc_filter = args.get("dcc").and_then(Value::as_str);
    let target_instance = args.get("instance_id").and_then(Value::as_str);

    match tool {
        "load_skill" | "unload_skill" => {
            match resolve_target(gs, target_instance, dcc_filter).await {
                Ok(entry) => {
                    // Strip gateway-only routing keys before forwarding.
                    let mut forward_args = args.clone();
                    if let Some(obj) = forward_args.as_object_mut() {
                        obj.remove("instance_id");
                    }
                    let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                    match forward_tools_call(
                        &gs.http_client,
                        &url,
                        tool,
                        Some(forward_args),
                        None,
                        None,
                        gs.backend_timeout,
                    )
                    .await
                    {
                        Ok(mut result) => {
                            inject_instance_metadata(
                                &mut result,
                                &entry.instance_id,
                                &entry.dcc_type,
                            );
                            let is_error = result
                                .get("isError")
                                .and_then(Value::as_bool)
                                .unwrap_or(false);
                            let text = result
                                .get("content")
                                .and_then(Value::as_array)
                                .and_then(|arr| arr.first())
                                .and_then(|c| c.get("text"))
                                .and_then(Value::as_str)
                                .map(str::to_owned)
                                .unwrap_or_else(|| {
                                    serde_json::to_string_pretty(&result).unwrap_or_default()
                                });
                            (text, is_error)
                        }
                        Err(e) => (format!("Backend call failed: {e}"), true),
                    }
                }
                Err(msg) => (msg, true),
            }
        }
        _ => {
            // Fan-out aggregation.
            let targets = targets_for_fanout(gs, dcc_filter).await;
            if targets.is_empty() {
                return (
                    "No live DCC instances. Start dcc-mcp-server on the DCC you want to use."
                        .to_string(),
                    true,
                );
            }

            let client = &gs.http_client;
            let backend_timeout = gs.backend_timeout;
            let params = json!({"name": tool, "arguments": args});
            let futs = targets.iter().map(|entry| {
                let url = format!("http://{}:{}/mcp", entry.host, entry.port);
                let params = params.clone();
                async move {
                    let res = super::backend_client::call_backend(
                        client,
                        &url,
                        "tools/call",
                        Some(params),
                        None,
                        backend_timeout,
                    )
                    .await;
                    (entry.instance_id, entry.dcc_type.clone(), res)
                }
            });
            let results = join_all(futs).await;

            let merged: Vec<Value> = results
                .into_iter()
                .map(|(iid, dcc, res)| match res {
                    Ok(v) => {
                        // Extract the actual text payload from the backend
                        // CallToolResult so the merged response is readable
                        // without double-unwrapping.
                        let text = v
                            .get("content")
                            .and_then(Value::as_array)
                            .and_then(|arr| arr.first())
                            .and_then(|c| c.get("text"))
                            .and_then(Value::as_str)
                            .map(str::to_owned)
                            .unwrap_or_else(|| {
                                serde_json::to_string_pretty(&v).unwrap_or_default()
                            });
                        json!({
                            "instance_id": iid.to_string(),
                            "instance_short": instance_short(&iid),
                            "dcc_type": dcc,
                            "result": text,
                        })
                    }
                    Err(e) => json!({
                        "instance_id": iid.to_string(),
                        "instance_short": instance_short(&iid),
                        "dcc_type": dcc,
                        "error": e,
                    }),
                })
                .collect();

            (
                serde_json::to_string_pretty(&json!({"instances": merged})).unwrap_or_default(),
                false,
            )
        }
    }
}

// ── Helpers ────────────────────────────────────────────────────────────────

async fn live_backends(gs: &GatewayState) -> Vec<ServiceEntry> {
    let reg = gs.registry.read().await;
    gs.live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE)
        .collect()
}

async fn targets_for_fanout(gs: &GatewayState, dcc_filter: Option<&str>) -> Vec<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect()
}

async fn find_instance_by_prefix(gs: &GatewayState, prefix: &str) -> Option<ServiceEntry> {
    live_backends(gs)
        .await
        .into_iter()
        .find(|e| instance_short(&e.instance_id) == prefix)
}

async fn resolve_target(
    gs: &GatewayState,
    instance_id: Option<&str>,
    dcc_filter: Option<&str>,
) -> Result<ServiceEntry, String> {
    let candidates = live_backends(gs).await;

    // Exact or prefix match on instance_id.
    if let Some(iid) = instance_id {
        if let Some(e) = candidates.iter().find(|e| {
            let full = e.instance_id.to_string();
            full == iid || full.starts_with(iid) || instance_short(&e.instance_id) == iid
        }) {
            return Ok(e.clone());
        }
        return Err(format!("No live instance matches instance_id='{iid}'"));
    }

    // DCC-filtered auto-select when unambiguous.
    let filtered: Vec<&ServiceEntry> = candidates
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type.eq_ignore_ascii_case(f)))
        .collect();

    match filtered.len() {
        0 => Err(match dcc_filter {
            Some(d) => format!("No live '{d}' instance."),
            None => "No live DCC instances.".to_string(),
        }),
        1 => Ok(filtered[0].clone()),
        _ => Err(format!(
            "Ambiguous target — {} instances live. Pass `instance_id` (or use `dcc` filter if only one of that type).",
            filtered.len()
        )),
    }
}

fn to_text_result(res: Result<String, String>) -> (String, bool) {
    match res {
        Ok(text) => (text, false),
        Err(msg) => (msg, true),
    }
}

fn inject_instance_metadata(value: &mut Value, iid: &Uuid, dcc_type: &str) {
    if let Some(obj) = value.as_object_mut() {
        obj.insert("_instance_id".to_string(), Value::String(iid.to_string()));
        obj.insert(
            "_instance_short".to_string(),
            Value::String(instance_short(iid)),
        );
        obj.insert("_dcc_type".to_string(), Value::String(dcc_type.to_string()));
    }
}

// ── Tools-list fingerprint (for SSE tools/list_changed aggregation) ─────────

/// Compute a fingerprint of the aggregated tool list across every live backend.
///
/// The fingerprint is a stable, sorted concatenation of `{instance_id}:{tool_name}`
/// across every live backend.  When this string changes between two polls, we
/// know at least one backend's tool list mutated (skill loaded / unloaded) and
/// we can push a single `notifications/tools/list_changed` to all connected
/// gateway SSE clients.
///
/// Deliberately excludes tool descriptions / schemas: we only want to detect
/// set-level add/remove changes, not metadata edits.
pub async fn compute_tools_fingerprint(
    registry: &std::sync::Arc<
        tokio::sync::RwLock<dcc_mcp_transport::discovery::file_registry::FileRegistry>,
    >,
    stale_timeout: Duration,
    http_client: &reqwest::Client,
    backend_timeout: Duration,
) -> String {
    let instances: Vec<ServiceEntry> = {
        let reg = registry.read().await;
        reg.list_all()
            .into_iter()
            .filter(|e| {
                !e.is_stale(stale_timeout)
                    && e.dcc_type != super::GATEWAY_SENTINEL_DCC_TYPE
                    && !matches!(
                        e.status,
                        dcc_mcp_transport::discovery::types::ServiceStatus::ShuttingDown
                            | dcc_mcp_transport::discovery::types::ServiceStatus::Unreachable
                    )
            })
            .collect()
    };

    let futs = instances.iter().map(|entry| async move {
        let url = format!("http://{}:{}/mcp", entry.host, entry.port);
        let tools = fetch_tools(http_client, &url, backend_timeout).await;
        (entry.instance_id, tools)
    });
    let results = join_all(futs).await;

    let mut parts: Vec<String> = results
        .into_iter()
        .flat_map(|(iid, tools)| {
            tools
                .into_iter()
                .map(move |t| format!("{iid}:{}", t.name))
                .collect::<Vec<_>>()
        })
        .collect();
    parts.sort_unstable();
    parts.join("|")
}

// ── Skill-management tool schemas ──────────────────────────────────────────

/// JSON-Schema definitions for the six skill-management tools the gateway
/// exposes (matching the per-DCC server schemas but with gateway-specific
/// routing parameters like `instance_id` and `dcc`).
fn skill_management_tool_defs() -> Vec<Value> {
    vec![
        json!({
            "name": "list_skills",
            "description": "List all skills across every live DCC instance. Returns a per-instance breakdown.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "status": {"type": "string", "enum": ["all", "loaded", "unloaded", "error"], "default": "all"},
                    "dcc":    {"type": "string", "description": "Restrict to one DCC type (maya, blender, …)"}
                }
            }
        }),
        json!({
            "name": "find_skills",
            "description": "DEPRECATED — use `search_skills`. Compat alias that forwards to `search_skills` \
                            on every live DCC instance. Scheduled for removal in v0.17.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "tags":  {"type": "array", "items": {"type": "string"}},
                    "dcc":   {"type": "string"}
                }
            }
        }),
        json!({
            "name": "search_skills",
            "description": "Unified skill discovery across every live DCC instance. Matches `query` against \
                            name/description/search_hint/tool names and filters by `tags`, `dcc`, `scope`. \
                            Call with no arguments to browse by trust scope (Admin > System > User > Repo).",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "query": {"type": "string"},
                    "tags":  {"type": "array", "items": {"type": "string"}},
                    "dcc":   {"type": "string"},
                    "scope": {"type": "string", "enum": ["repo", "user", "system", "admin"]},
                    "limit": {"type": "integer", "minimum": 1, "maximum": 100, "default": 20}
                }
            }
        }),
        json!({
            "name": "get_skill_info",
            "description": "Get detailed skill info (tools, scripts, dependencies) from each instance that has it.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name": {"type": "string"},
                    "dcc":        {"type": "string"}
                },
                "required": ["skill_name"]
            }
        }),
        json!({
            "name": "load_skill",
            "description": "Load a skill on a specific DCC instance. When multiple instances are live, \
                            pass `instance_id` (or the short prefix from list_dcc_instances). With a single \
                            live instance the routing is automatic.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name":  {"type": "string"},
                    "skill_names": {"type": "array", "items": {"type": "string"}},
                    "instance_id": {"type": "string", "description": "Target instance (full UUID or short prefix)"},
                    "dcc":         {"type": "string", "description": "DCC type when only one instance of that type is live"}
                },
                "required": ["skill_name"]
            }
        }),
        json!({
            "name": "unload_skill",
            "description": "Unload a skill on a specific DCC instance. Same routing rules as load_skill.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "skill_name":  {"type": "string"},
                    "instance_id": {"type": "string"},
                    "dcc":         {"type": "string"}
                },
                "required": ["skill_name"]
            }
        }),
    ]
}

// ── Unit tests ────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn skill_management_tool_defs_cover_all_six_tools() {
        let defs = skill_management_tool_defs();
        let names: Vec<&str> = defs
            .iter()
            .filter_map(|v| v.get("name").and_then(|n| n.as_str()))
            .collect();
        for expected in [
            "list_skills",
            "find_skills",
            "search_skills",
            "get_skill_info",
            "load_skill",
            "unload_skill",
        ] {
            assert!(names.contains(&expected), "missing tool def {expected}");
        }
        assert_eq!(defs.len(), 6, "expected exactly 6 skill-management tools");
    }

    #[test]
    fn skill_management_tool_defs_all_declare_input_schema() {
        for def in skill_management_tool_defs() {
            let schema = def.get("inputSchema").expect("inputSchema present");
            assert_eq!(
                schema.get("type").and_then(|v| v.as_str()),
                Some("object"),
                "schema for {} is not an object",
                def.get("name").unwrap()
            );
        }
    }

    #[test]
    fn inject_instance_metadata_adds_annotations_to_object() {
        let id = Uuid::parse_str("abcdef0123456789abcdef0123456789").unwrap();
        let mut value = json!({"existing": "field"});
        inject_instance_metadata(&mut value, &id, "maya");

        let obj = value.as_object().unwrap();
        assert_eq!(obj.get("existing").unwrap(), &json!("field"));
        assert_eq!(obj.get("_instance_id").unwrap(), &json!(id.to_string()));
        assert_eq!(obj.get("_instance_short").unwrap(), &json!("abcdef01"));
        assert_eq!(obj.get("_dcc_type").unwrap(), &json!("maya"));
    }

    #[test]
    fn inject_instance_metadata_is_noop_for_non_objects() {
        let id = Uuid::new_v4();
        // Arrays and scalars cannot receive annotations — the helper must
        // silently skip them rather than panic.
        let mut arr = json!([1, 2, 3]);
        inject_instance_metadata(&mut arr, &id, "blender");
        assert_eq!(arr, json!([1, 2, 3]));

        let mut s = json!("scalar");
        inject_instance_metadata(&mut s, &id, "blender");
        assert_eq!(s, json!("scalar"));
    }

    #[test]
    fn to_text_result_maps_ok_to_success() {
        let (text, is_error) = to_text_result(Ok("payload".to_string()));
        assert_eq!(text, "payload");
        assert!(!is_error);
    }

    #[test]
    fn to_text_result_maps_err_to_error() {
        let (text, is_error) = to_text_result(Err("boom".to_string()));
        assert_eq!(text, "boom");
        assert!(is_error);
    }
}
