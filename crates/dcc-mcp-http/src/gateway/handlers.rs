//! Axum request handlers for the gateway HTTP server.

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::{IntoResponse, Response};
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::gateway::is_newer_version;

use super::proxy::proxy_request;
use super::state::{GatewayState, entry_to_json};
use super::tools::{
    gateway_tool_defs, tool_connect_to_dcc, tool_get_instance, tool_list_instances,
};
use dcc_mcp_transport::discovery::types::ServiceStatus;

/// Minimal JSON-RPC 2.0 request shape accepted by the gateway `/mcp` endpoint.
#[derive(Debug, Deserialize)]
pub struct JsonRpcRequest {
    #[allow(dead_code)]
    pub jsonrpc: Option<String>,
    pub id: Option<Value>,
    pub method: String,
    pub params: Option<Value>,
}

// ── REST handlers ─────────────────────────────────────────────────────────────

/// `GET /health` — simple liveness probe.
pub async fn handle_health() -> impl IntoResponse {
    Json(json!({"ok": true, "service": "dcc-mcp-gateway"}))
}

/// `POST /gateway/yield` — ask this gateway to voluntarily release its port.
///
/// The requester must supply its own version in the JSON body.
/// The gateway only yields if the requester's version is strictly newer,
/// preventing accidental downgrades.
///
/// # Request body
/// ```json
/// { "challenger_version": "0.12.29" }
/// ```
///
/// # Responses
/// - `200 OK` — yield accepted; gateway is shutting down.
/// - `409 Conflict` — challenger version is not newer than the gateway.
/// - `400 Bad Request` — malformed body.
pub async fn handle_gateway_yield(
    State(gs): State<super::state::GatewayState>,
    body: axum::body::Bytes,
) -> Response {
    #[derive(Deserialize)]
    struct YieldRequest {
        challenger_version: String,
    }

    let req: YieldRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"ok": false, "error": format!("Invalid body: {e}")})),
            )
                .into_response();
        }
    };

    if is_newer_version(&req.challenger_version, &gs.server_version) {
        tracing::info!(
            challenger = %req.challenger_version,
            current = %gs.server_version,
            "Gateway yield requested — initiating graceful handoff"
        );
        // Signal the gateway HTTP server to shut down gracefully.
        // The port will be freed once axum drains in-flight requests.
        let _ = gs.yield_tx.send(true);
        Json(json!({
            "ok": true,
            "message": format!(
                "Gateway v{} yielding to challenger v{}. Port will be free shortly.",
                gs.server_version, req.challenger_version
            )
        }))
        .into_response()
    } else {
        (
            StatusCode::CONFLICT,
            Json(json!({
                "ok": false,
                "error": format!(
                    "Challenger version {} is not newer than gateway {}. Yield refused.",
                    req.challenger_version, gs.server_version
                )
            })),
        )
            .into_response()
    }
}

/// `GET /instances` — return all live instances as JSON.
pub async fn handle_instances(State(gs): State<GatewayState>) -> impl IntoResponse {
    let reg = gs.registry.read().await;
    let instances: Vec<Value> = gs
        .live_instances(&reg)
        .into_iter()
        .map(|e| entry_to_json(&e, gs.stale_timeout))
        .collect();
    Json(json!({ "total": instances.len(), "instances": instances }))
}

// ── MCP endpoint ──────────────────────────────────────────────────────────────

/// `POST /mcp` — gateway's own MCP endpoint with discovery meta-tools.
/// Does NOT proxy; returns direct URLs for agents to use.
pub async fn handle_gateway_mcp(
    State(gs): State<GatewayState>,
    body: axum::body::Bytes,
) -> Response {
    let req: JsonRpcRequest = match serde_json::from_slice(&body) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {e}")}})),
            )
                .into_response();
        }
    };

    let id = req.id.clone();
    let resp = match req.method.as_str() {
        "initialize" => json!({
            "jsonrpc": "2.0", "id": id,
            "result": {
                "protocolVersion": "2025-03-26",
                "capabilities": {"tools": {"listChanged": false}},
                "serverInfo": {"name": gs.server_name, "version": gs.server_version},
                "instructions":
                    "DCC-MCP Gateway — multi-instance discovery.\n\
                     1. Call list_dcc_instances to see all running DCC servers.\n\
                     2. Call connect_to_dcc to get the MCP URL for a specific DCC type.\n\
                     3. Connect your MCP client directly to that URL for zero-overhead access.\n\
                     4. Or use POST /mcp/{instance_id} on this gateway for transparent proxying."
            }
        }),
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "notifications/initialized" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => {
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"tools": gateway_tool_defs(), "nextCursor": null}
            })
        }
        "tools/call" => {
            let tool = req
                .params
                .as_ref()
                .and_then(|p| p.get("name"))
                .and_then(|n| n.as_str())
                .unwrap_or("");
            let args = req
                .params
                .as_ref()
                .and_then(|p| p.get("arguments"))
                .cloned()
                .unwrap_or(json!({}));

            let result = match tool {
                "list_dcc_instances" => tool_list_instances(&gs, &args).await,
                "get_dcc_instance" => tool_get_instance(&gs, &args).await,
                "connect_to_dcc" => tool_connect_to_dcc(&gs, &args).await,
                other => Err(format!("Unknown tool: {other}")),
            };

            match result {
                Ok(text) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": text}], "isError": false}
                }),
                Err(msg) => json!({
                    "jsonrpc": "2.0", "id": id,
                    "result": {"content": [{"type": "text", "text": msg}], "isError": true}
                }),
            }
        }
        other => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        }),
    };

    let mut response = Json(resp).into_response();
    response
        .headers_mut()
        .insert("Mcp-Session-Id", "dcc-mcp-gateway".parse().unwrap());
    response
}

// ── Proxy handlers ────────────────────────────────────────────────────────────

/// `POST /mcp/{instance_id}` — transparent proxy to a specific DCC instance.
pub async fn handle_proxy_instance(
    State(gs): State<GatewayState>,
    Path(instance_id): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let reg = gs.registry.read().await;
    let entry = reg.list_all().into_iter().find(|e| {
        let eid = e.instance_id.to_string();
        eid == instance_id || eid.starts_with(&instance_id)
    });
    drop(reg);

    match entry {
        Some(e) => {
            let url = format!("http://{}:{}/mcp", e.host, e.port);
            proxy_request(&gs.http_client, &url, headers, body).await
        }
        None => (
            StatusCode::NOT_FOUND,
            Json(json!({"error": format!("Instance '{}' not found", instance_id)})),
        )
            .into_response(),
    }
}

/// `POST /mcp/dcc/{dcc_type}` — proxy to best available instance of a DCC type.
pub async fn handle_proxy_dcc(
    State(gs): State<GatewayState>,
    Path(dcc_type): Path<String>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    let reg = gs.registry.read().await;
    let mut candidates = gs
        .live_instances(&reg)
        .into_iter()
        .filter(|e| e.dcc_type == dcc_type)
        .collect::<Vec<_>>();
    drop(reg);

    if candidates.is_empty() {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({"error": format!("No live '{}' instances", dcc_type)})),
        )
            .into_response();
    }

    // Prefer Available over Busy
    candidates.sort_by_key(|e| matches!(e.status, ServiceStatus::Busy) as u8);
    let url = format!("http://{}:{}/mcp", candidates[0].host, candidates[0].port);
    proxy_request(&gs.http_client, &url, headers, body).await
}
