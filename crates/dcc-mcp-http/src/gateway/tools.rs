//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};
use dcc_mcp_transport::discovery::types::ServiceEntry;

// ── helpers ────────────────────────────────────────────────────────────────

/// Return true when a `scene` hint matches the instance's active document or
/// any of its open `documents` (case-insensitive substring match).
fn scene_matches(e: &ServiceEntry, hint: &str) -> bool {
    let lower = hint.to_lowercase();
    e.scene
        .as_deref()
        .is_some_and(|s| s.to_lowercase().contains(&lower))
        || e.documents
            .iter()
            .any(|d| d.to_lowercase().contains(&lower))
}

/// Return true when a `document` hint matches any open document
/// (case-insensitive substring match).  Used by Photoshop-style apps.
fn document_matches(e: &ServiceEntry, hint: &str) -> bool {
    let lower = hint.to_lowercase();
    e.documents
        .iter()
        .any(|d| d.to_lowercase().contains(&lower))
        || e.scene
            .as_deref()
            .is_some_and(|s| s.to_lowercase().contains(&lower))
}

// ── tools ──────────────────────────────────────────────────────────────────

/// `list_dcc_instances` — list every parseable DCC server registered in the
/// shared registry, optionally filtered by `dcc_type`.
///
/// Issue maya#138: prior to this change the tool reused
/// [`GatewayState::live_instances`], which silently dropped stale rows,
/// shutting-down rows, and any registration with `dcc_type == "unknown"`.
/// Operators inspecting `$TEMP/dcc-mcp-registry/` then saw three sentinels
/// on disk but only one row in the tool output, with no signal as to why
/// the others vanished — making it nearly impossible to diagnose why their
/// Maya plugin was missing or why the standalone server appeared to "win"
/// the gateway.  The tool now surfaces:
///
/// * Every parseable row except the bookkeeping `__gateway__` sentinel
///   and the gateway's own self-row.
/// * Stale rows with `status: "stale"` so callers can render them as
///   crashed/abandoned without dropping them silently.
/// * `unknown` rows unconditionally, since this view is operator-facing
///   and the existing `allow_unknown_tools` guard still governs whether
///   their tools are routable through the gateway façade.
///
/// Pass `include_stale: false` (boolean) to opt out of stale rows for
/// callers that genuinely want only routable instances.
pub async fn tool_list_instances(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_filter = args.get("dcc_type").and_then(|v| v.as_str());
    let include_stale = args
        .get("include_stale")
        .and_then(|v| v.as_bool())
        .unwrap_or(true);

    let reg = gs.registry.read().await;
    let raw = gs.all_instances(&reg);

    let mut stale_count: usize = 0;
    let mut instances: Vec<Value> = raw
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type == f))
        .filter(|e| {
            let stale = e.is_stale(gs.stale_timeout);
            if stale {
                stale_count += 1;
            }
            include_stale || !stale
        })
        .map(|e| entry_to_json(e, gs.stale_timeout))
        .collect();

    instances.sort_by(|a, b| {
        a["dcc_type"]
            .as_str()
            .cmp(&b["dcc_type"].as_str())
            .then(a["port"].as_u64().cmp(&b["port"].as_u64()))
    });

    let tip = if instances.is_empty() {
        "No DCC instances in the registry. Start dcc-mcp-server for each DCC application."
    } else if stale_count > 0 && include_stale {
        "Some rows have status='stale' (no recent heartbeat). \
         Use connect_to_dcc(dcc_type=..., scene=...) to route to a live one — \
         pass `scene`, `document`, `display_name`, or `instance_id` to disambiguate."
    } else {
        "Use connect_to_dcc(dcc_type=..., scene=...) to get the direct MCP URL. \
         When multiple instances of the same DCC type are running, pass `scene`, \
         `document`, `display_name`, or `instance_id` to select one."
    };

    serde_json::to_string_pretty(&json!({
        "total":        instances.len(),
        "stale_count":  stale_count,
        "instances":    instances,
        "tip":          tip,
    }))
    .map_err(|e| e.to_string())
}

/// `get_dcc_instance` — get details for a specific instance.
///
/// Selection priority (first match wins):
/// 1. `instance_id` — exact UUID or unique prefix
/// 2. `dcc_type` + `display_name` — label set by the bridge plugin
/// 3. `dcc_type` + `scene` / `document` — substring match against active scene and all open docs
/// 4. `dcc_type` alone — returns immediately when only 1 instance exists;
///    when >1 exist, returns a disambiguation object instead of silently picking the first.
pub async fn tool_get_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

    // ── 1. Exact instance_id ──────────────────────────────────────────────
    if let Some(id) = args.get("instance_id").and_then(|v| v.as_str()) {
        return all
            .iter()
            .find(|e| {
                let s = e.instance_id.to_string();
                s == id || s.starts_with(id)
            })
            .map(|e| {
                serde_json::to_string_pretty(&entry_to_json(e, gs.stale_timeout))
                    .unwrap_or_default()
            })
            .ok_or_else(|| format!("Instance '{id}' not found"));
    }

    // ── 2-4. dcc_type-scoped search ───────────────────────────────────────
    if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!("No live '{dcc}' instances"));
        }

        // display_name match
        if let Some(name) = args.get("display_name").and_then(|v| v.as_str()) {
            if let Some(e) = candidates.iter().find(|e| {
                e.display_name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(&name.to_lowercase()))
            }) {
                return serde_json::to_string_pretty(&entry_to_json(e, gs.stale_timeout))
                    .map_err(|e| e.to_string());
            }
        }

        // scene / document hint
        let scene_hint = args
            .get("scene")
            .or_else(|| args.get("document"))
            .and_then(|v| v.as_str());
        if let Some(hint) = scene_hint {
            if let Some(e) = candidates
                .iter()
                .find(|e| scene_matches(e, hint) || document_matches(e, hint))
            {
                return serde_json::to_string_pretty(&entry_to_json(e, gs.stale_timeout))
                    .map_err(|e| e.to_string());
            }
        }

        // Single unambiguous candidate
        if candidates.len() == 1 {
            return serde_json::to_string_pretty(&entry_to_json(candidates[0], gs.stale_timeout))
                .map_err(|e| e.to_string());
        }

        // Multiple candidates — ask the agent to disambiguate
        return build_disambiguation(candidates, dcc, gs);
    }

    Err("Provide instance_id or dcc_type".to_string())
}

/// `connect_to_dcc` — return the direct MCP URL for a DCC instance.
///
/// Same selection priority as `get_dcc_instance`.  When multiple instances match
/// and no hint narrows the result to one, returns a structured
/// `disambiguation_required` object that the agent should present to the user
/// before retrying with `instance_id`.
pub async fn tool_connect_to_dcc(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

    // ── 1. Exact instance_id ──────────────────────────────────────────────
    if let Some(id) = args.get("instance_id").and_then(|v| v.as_str()) {
        let entry = all
            .iter()
            .find(|e| {
                let s = e.instance_id.to_string();
                s == id || s.starts_with(id)
            })
            .cloned()
            .ok_or_else(|| format!("Instance '{id}' not found"))?;
        return format_connect_response(&entry);
    }

    // ── 2-4. dcc_type-scoped search ───────────────────────────────────────
    if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!(
                "No live '{dcc}' instances. Start: dcc-mcp-server --dcc {dcc}"
            ));
        }

        // display_name match
        if let Some(name) = args.get("display_name").and_then(|v| v.as_str()) {
            if let Some(e) = candidates.iter().find(|e| {
                e.display_name
                    .as_deref()
                    .is_some_and(|n| n.to_lowercase().contains(&name.to_lowercase()))
            }) {
                return format_connect_response(e);
            }
        }

        // scene / document hint
        let scene_hint = args
            .get("scene")
            .or_else(|| args.get("document"))
            .and_then(|v| v.as_str());
        if let Some(hint) = scene_hint {
            if let Some(e) = candidates
                .iter()
                .find(|e| scene_matches(e, hint) || document_matches(e, hint))
            {
                return format_connect_response(e);
            }
        }

        // Single unambiguous candidate
        if candidates.len() == 1 {
            return format_connect_response(candidates[0]);
        }

        // Multiple candidates — must disambiguate
        return build_disambiguation(candidates, dcc, gs);
    }

    Err("Provide instance_id or dcc_type".to_string())
}

/// Gateway-native `diagnostics__process_status`.
pub async fn tool_diagnostics_process_status(
    gs: &GatewayState,
    args: &Value,
) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.all_instances(&reg);
    let dcc_filter = args.get("dcc_type").and_then(|v| v.as_str());

    let mut live_count = 0usize;
    let mut stale_count = 0usize;
    let mut unhealthy_count = 0usize;
    let instances: Vec<Value> = all
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type == f))
        .map(|e| {
            let stale = e.is_stale(gs.stale_timeout);
            if stale {
                stale_count += 1;
            } else if matches!(
                e.status,
                dcc_mcp_transport::discovery::types::ServiceStatus::Available
                    | dcc_mcp_transport::discovery::types::ServiceStatus::Busy
            ) {
                live_count += 1;
            } else {
                unhealthy_count += 1;
            }
            entry_to_json(e, gs.stale_timeout)
        })
        .collect();

    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway process status",
        "gateway": {
            "server_name": gs.server_name,
            "server_version": gs.server_version,
            "own_host": gs.own_host,
            "own_port": gs.own_port,
        },
        "instances": instances,
        "counts": {
            "total": instances.len(),
            "live": live_count,
            "stale": stale_count,
            "unhealthy": unhealthy_count,
        }
    }))
    .map_err(|e| e.to_string())
}

/// Gateway-native `diagnostics__audit_log`.
pub async fn tool_diagnostics_audit_log(
    gs: &GatewayState,
    _args: &Value,
) -> Result<String, String> {
    let pending_calls = gs.pending_calls.read().await.len();
    let subscriptions = gs.resource_subscriptions.read().await.len();
    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway audit summary",
        "entries": [],
        "summary": {
            "pending_calls": pending_calls,
            "resource_subscription_sessions": subscriptions,
            "note": "Gateway-native audit history is not persisted; backend audit logs remain available via prefixed diagnostics tools when exposed by a DCC instance."
        }
    }))
    .map_err(|e| e.to_string())
}

/// Gateway-native `diagnostics__tool_metrics`.
pub async fn tool_diagnostics_tool_metrics(
    gs: &GatewayState,
    _args: &Value,
) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let live_instances = gs.live_instances(&reg);
    serde_json::to_string_pretty(&json!({
        "success": true,
        "message": "Gateway tool metrics summary",
        "metrics": {
            "gateway_local_tools": gateway_tool_defs().as_array().map_or(0, Vec::len),
            "live_instances": live_instances.len(),
            "backend_timeout_ms": gs.backend_timeout.as_millis(),
            "async_dispatch_timeout_ms": gs.async_dispatch_timeout.as_millis(),
        }
    }))
    .map_err(|e| e.to_string())
}

// ── private helpers ────────────────────────────────────────────────────────

fn format_connect_response(entry: &ServiceEntry) -> Result<String, String> {
    let mcp_url = format!("http://{}:{}/mcp", entry.host, entry.port);
    let id = entry.instance_id;
    serde_json::to_string_pretty(&json!({
        "instance_id":  id.to_string(),
        "dcc_type":     entry.dcc_type,
        "mcp_url":      mcp_url,
        "scene":        entry.scene,
        "documents":    entry.documents,
        "pid":          entry.pid,
        "display_name": entry.display_name,
        "status":       entry.status.to_string(),
        "instructions": format!(
            "Point your MCP client to: {mcp_url}\n\
             Direct connection = zero proxy overhead.\n\
             Or use POST /mcp/{id} on this gateway for transparent proxying."
        )
    }))
    .map_err(|e| e.to_string())
}

/// Build a structured disambiguation response.
///
/// The response signals `disambiguation_required: true` so the agent can present
/// the list to the user and ask which instance to operate on, then retry with the
/// chosen `instance_id`.
fn build_disambiguation(
    candidates: Vec<&ServiceEntry>,
    dcc: &str,
    gs: &GatewayState,
) -> Result<String, String> {
    let choices: Vec<Value> = candidates
        .iter()
        .map(|e| {
            let label = e
                .display_name
                .clone()
                .or_else(|| {
                    e.scene.as_ref().map(|s| {
                        // Show just the filename portion for readability
                        std::path::Path::new(s)
                            .file_name()
                            .and_then(|n| n.to_str())
                            .unwrap_or(s.as_str())
                            .to_string()
                    })
                })
                .unwrap_or_else(|| format!("{}:{}", e.host, e.port));
            let mut j = entry_to_json(e, gs.stale_timeout);
            j["label"] = json!(label);
            j
        })
        .collect();

    serde_json::to_string_pretty(&json!({
        "disambiguation_required": true,
        "dcc_type": dcc,
        "message": format!(
            "Found {} '{}' instances. Ask the user which one to use, \
             then retry with the chosen instance_id.",
            choices.len(), dcc
        ),
        "hint": "Pass `display_name`, `scene`, or `instance_id` to connect_to_dcc \
                 to select a specific instance without asking the user.",
        "instances": choices
    }))
    .map_err(|e| e.to_string())
}

/// Return the JSON schema for the three gateway discovery tools.
pub fn gateway_tool_defs() -> serde_json::Value {
    json!([
        {
            "name": "list_dcc_instances",
            "description": "List every DCC server instance registered in the shared registry. \
                Returns type, port, scene, documents, pid, display_name, version, adapter_version, \
                adapter_dcc, and status. Stale rows (no recent heartbeat) are reported with \
                status='stale' so operators can see why a registration is no longer routable; \
                pass include_stale=false to hide them. Call this first to discover what's available.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dcc_type": {
                        "type": "string",
                        "description": "Filter by DCC type (e.g. 'maya', 'photoshop'). Omit for all."
                    },
                    "include_stale": {
                        "type": "boolean",
                        "description": "Include rows with status='stale' (default: true). Set to false for routable-only output.",
                        "default": true
                    }
                }
            }
        },
        {
            "name": "get_dcc_instance",
            "description": "Get full details for a specific DCC instance. \
                When multiple instances of the same type exist, pass a hint to select one: \
                use `display_name` (e.g. 'Maya-Rig'), `scene` / `document` (filename substring), \
                or `instance_id` (exact UUID). \
                If no hint resolves to a single instance, a `disambiguation_required` object \
                is returned — show the list to the user and ask which one to use.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id":   {"type": "string", "description": "UUID (or unique prefix) from list_dcc_instances"},
                    "dcc_type":      {"type": "string", "description": "DCC type (e.g. 'maya')"},
                    "scene":         {"type": "string", "description": "Substring of the active scene filename"},
                    "document":      {"type": "string", "description": "Substring of any open document (multi-doc apps like Photoshop)"},
                    "display_name":  {"type": "string", "description": "Human-readable label set by the bridge plugin (e.g. 'Maya-Rigging')"}
                }
            }
        },
        {
            "name": "connect_to_dcc",
            "description": "Get the direct MCP URL for a DCC instance and connect your client to it. \
                Same selection logic as get_dcc_instance. \
                IMPORTANT: when `disambiguation_required` is true in the response, \
                show the instance list to the user, get their choice, then call again with `instance_id`.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id":   {"type": "string", "description": "UUID (or unique prefix)"},
                    "dcc_type":      {"type": "string", "description": "DCC type (e.g. 'maya', 'photoshop')"},
                    "scene":         {"type": "string", "description": "Substring of the active scene filename"},
                    "document":      {"type": "string", "description": "Substring of any open document (multi-doc apps)"},
                    "display_name":  {"type": "string", "description": "Human-readable label set by the bridge plugin"}
                }
            }
        },
        {
            "name": "diagnostics__process_status",
            "description": "Gateway-native process and instance health summary. Lists live, stale, and unhealthy DCC registrations without requiring a backend instance.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dcc_type": {"type": "string", "description": "Optional DCC type filter."}
                }
            }
        },
        {
            "name": "diagnostics__audit_log",
            "description": "Gateway-native audit summary for pending calls and resource subscriptions. Backend audit logs remain available through instance tools.",
            "inputSchema": {"type": "object", "properties": {}}
        },
        {
            "name": "diagnostics__tool_metrics",
            "description": "Gateway-native tool metrics summary for local gateway tools and live backend count.",
            "inputSchema": {"type": "object", "properties": {}}
        }
    ])
}
