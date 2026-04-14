//! MCP discovery meta-tools served by the gateway's `/mcp` endpoint.

use serde_json::{Value, json};

use super::state::{GatewayState, entry_to_json};
use dcc_mcp_transport::discovery::types::ServiceEntry;

/// `list_dcc_instances` — list all live DCC servers, optionally filtered by type.
pub async fn tool_list_instances(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let dcc_filter = args.get("dcc_type").and_then(|v| v.as_str());
    let reg = gs.registry.read().await;
    let mut instances: Vec<Value> = gs
        .live_instances(&reg)
        .iter()
        .filter(|e| dcc_filter.is_none_or(|f| e.dcc_type == f))
        .map(|e| entry_to_json(e, gs.stale_timeout))
        .collect();

    instances.sort_by(|a, b| {
        a["dcc_type"]
            .as_str()
            .cmp(&b["dcc_type"].as_str())
            .then(a["port"].as_u64().cmp(&b["port"].as_u64()))
    });

    let tip = if instances.is_empty() {
        "No live DCC instances. Start dcc-mcp-server for each DCC application."
    } else {
        "Use connect_to_dcc(dcc_type=...) to get the direct MCP URL. \
         Connect your agent directly — no proxy needed."
    };

    serde_json::to_string_pretty(&json!({
        "total": instances.len(),
        "instances": instances,
        "tip": tip
    }))
    .map_err(|e| e.to_string())
}

/// `get_dcc_instance` — get details for a specific instance by id or dcc_type+scene.
pub async fn tool_get_instance(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

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

    if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!("No live '{dcc}' instances"));
        }
        let scene = args.get("scene").and_then(|v| v.as_str());
        let entry = scene
            .and_then(|hint| {
                candidates
                    .iter()
                    .find(|e| e.scene.as_deref().unwrap_or("").contains(hint))
            })
            .copied()
            .unwrap_or(candidates[0]);
        return serde_json::to_string_pretty(&entry_to_json(entry, gs.stale_timeout))
            .map_err(|e| e.to_string());
    }

    Err("Provide instance_id or dcc_type".to_string())
}

/// `connect_to_dcc` — return the direct MCP URL for a DCC instance.
pub async fn tool_connect_to_dcc(gs: &GatewayState, args: &Value) -> Result<String, String> {
    let reg = gs.registry.read().await;
    let all = gs.live_instances(&reg);

    let entry = if let Some(id) = args.get("instance_id").and_then(|v| v.as_str()) {
        all.iter()
            .find(|e| {
                let s = e.instance_id.to_string();
                s == id || s.starts_with(id)
            })
            .cloned()
            .ok_or_else(|| format!("Instance '{id}' not found"))?
    } else if let Some(dcc) = args.get("dcc_type").and_then(|v| v.as_str()) {
        let candidates: Vec<&ServiceEntry> = all.iter().filter(|e| e.dcc_type == dcc).collect();
        if candidates.is_empty() {
            return Err(format!(
                "No live '{dcc}' instances. Start: dcc-mcp-server --dcc {dcc}"
            ));
        }
        let scene = args.get("scene").and_then(|v| v.as_str());
        let e = scene
            .and_then(|h| {
                candidates
                    .iter()
                    .find(|e| e.scene.as_deref().unwrap_or("").contains(h))
            })
            .copied()
            .unwrap_or(candidates[0]);
        e.clone()
    } else {
        return Err("Provide instance_id or dcc_type".to_string());
    };

    let mcp_url = format!("http://{}:{}/mcp", entry.host, entry.port);
    serde_json::to_string_pretty(&json!({
        "instance_id": entry.instance_id.to_string(),
        "dcc_type": entry.dcc_type,
        "mcp_url": mcp_url,
        "scene": entry.scene,
        "status": entry.status.to_string(),
        "instructions": format!(
            "Point your MCP client to: {mcp_url}\n\
             Direct connection = zero proxy overhead.\n\
             Or use POST /mcp/{id} on this gateway for transparent proxying.",
            id = entry.instance_id
        )
    }))
    .map_err(|e| e.to_string())
}

/// Return the JSON schema for the three gateway discovery tools.
pub fn gateway_tool_defs() -> serde_json::Value {
    json!([
        {
            "name": "list_dcc_instances",
            "description": "List all running DCC server instances. Returns type, port, scene, status.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "dcc_type": {"type": "string", "description": "Filter by type (e.g. 'maya'). Omit for all."}
                }
            }
        },
        {
            "name": "get_dcc_instance",
            "description": "Get info on a specific DCC instance by id or dcc_type+scene.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {"type": "string", "description": "UUID (or prefix) from list_dcc_instances"},
                    "dcc_type": {"type": "string"},
                    "scene": {"type": "string", "description": "Scene name hint for selection"}
                }
            }
        },
        {
            "name": "connect_to_dcc",
            "description": "Get the direct MCP URL for a DCC instance. Connect to it directly for zero-overhead access.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "instance_id": {"type": "string"},
                    "dcc_type": {"type": "string"},
                    "scene": {"type": "string"}
                }
            }
        }
    ])
}
