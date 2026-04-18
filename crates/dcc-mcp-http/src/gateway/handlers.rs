//! Axum request handlers for the gateway HTTP server.

use std::convert::Infallible;

use axum::Json;
use axum::extract::{Path, State};
use axum::http::{HeaderMap, StatusCode};
use axum::response::sse::{Event, KeepAlive, Sse};
use axum::response::{IntoResponse, Response};
use futures::stream;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio_stream::StreamExt;
use tokio_stream::wrappers::BroadcastStream;

use super::super::gateway::is_newer_version;
use super::aggregator;
use super::proxy::proxy_request;
use super::state::{GatewayState, entry_to_json};
use crate::protocol::negotiate_protocol_version;
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

// ── SSE handler ───────────────────────────────────────────────────────────────

/// `GET /mcp` — server-sent event stream for MCP push notifications.
///
/// MCP clients that support the Streamable HTTP transport (2025-03-26 spec) open
/// this endpoint after `initialize` and keep it open to receive server-initiated
/// notifications without polling.
///
/// The gateway currently pushes:
/// - `notifications/resources/list_changed` — whenever the set of live DCC
///   instances changes (new instance joins, old one goes stale, etc.)
///
/// # Acceptance criteria
/// The request must carry `Accept: text/event-stream`; otherwise we return
/// `405 Method Not Allowed` to avoid confusing plain browser visits.
pub async fn handle_gateway_get(
    State(gs): State<super::state::GatewayState>,
    headers: HeaderMap,
) -> Response {
    let accepts_sse = headers
        .get(axum::http::header::ACCEPT)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.contains("text/event-stream"))
        .unwrap_or(false);

    if !accepts_sse {
        return (
            StatusCode::METHOD_NOT_ALLOWED,
            Json(json!({"error": "This endpoint streams SSE. Set Accept: text/event-stream"})),
        )
            .into_response();
    }

    let rx = gs.events_tx.subscribe();

    // Convert broadcast receiver into an SSE stream.
    // Lagged messages (receiver too slow) are skipped gracefully.
    let sse_stream = BroadcastStream::new(rx).filter_map(|result| {
        let data = match result {
            Ok(s) => s,
            Err(tokio_stream::wrappers::errors::BroadcastStreamRecvError::Lagged(n)) => {
                tracing::warn!("Gateway SSE: client lagged, skipped {n} message(s)");
                return None;
            }
        };
        Some(Ok::<Event, Infallible>(Event::default().data(data)))
    });

    // Prepend an endpoint-event so MCP clients know where to POST.
    // (Streamable HTTP spec §4.2 requires an initial `endpoint` event.)
    let endpoint_event = stream::once(async {
        Ok::<Event, Infallible>(Event::default().event("endpoint").data("/mcp"))
    });

    Sse::new(endpoint_event.chain(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response()
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

/// `POST /mcp` — gateway's own MCP endpoint (facade over every live DCC).
///
/// Aggregates `tools/list` from every backend, routes `tools/call` by the
/// instance-prefix encoded into each tool name, and handles the 3 discovery
/// meta-tools + 6 skill-management tools locally / with fan-out.
pub async fn handle_gateway_mcp(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
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

    // Preserve the client's session id (if any) so the SSE subscription opened
    // via GET /mcp can correlate with POST /mcp calls.
    let client_session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned);

    let id = req.id.clone();
    let resp = match req.method.as_str() {
        "initialize" => {
            // Negotiate protocol version with the client.
            let client_version = req
                .params
                .as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str());
            let negotiated = negotiate_protocol_version(client_version);

            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": negotiated,
                    "capabilities": {
                        // Aggregated tool list changes as backends load/unload skills.
                        "tools": {"listChanged": true},
                        // Resources (DCC instances) change dynamically.
                        // Clients should subscribe to GET /mcp SSE stream for push notifications.
                        "resources": {"listChanged": true, "subscribe": true}
                    },
                    "serverInfo": {"name": gs.server_name, "version": gs.server_version},
                    "instructions":
                        "DCC-MCP Gateway — unified MCP endpoint across every live DCC.\n\
                         \n\
                         tools/list returns:\n\
                         • 3 discovery meta-tools (list_dcc_instances / get_dcc_instance / connect_to_dcc)\n\
                         • 6 skill-management tools (list/find/search/get_info/load/unload_skill)\n\
                         • Every backend tool, prefixed with an 8-char instance id (e.g. abcd1234__maya_geometry__create_sphere)\n\
                         \n\
                         Workflow:\n\
                         1. search_skills(query=...) — find relevant skills across every live DCC\n\
                         2. load_skill(skill_name=..., instance_id=... when multiple instances exist)\n\
                         3. Call the prefixed tool directly through this same endpoint\n\
                         \n\
                         Subscribe to GET /mcp (SSE) for notifications/tools/list_changed and\n\
                         notifications/resources/list_changed push events."
                }
            })
        }
        "ping" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "notifications/initialized" => json!({"jsonrpc":"2.0","id":id,"result":{}}),
        "tools/list" => {
            let result = aggregator::aggregate_tools_list(&gs).await;
            json!({"jsonrpc": "2.0", "id": id, "result": result})
        }
        // ── MCP Resources API ─────────────────────────────────────────────
        // Each live DCC instance is a resource with URI: dcc://{dcc_type}/{instance_id}
        // Clients subscribe to the SSE stream (GET /mcp) to receive push notifications.
        "resources/list" => {
            let reg = gs.registry.read().await;
            let resources: Vec<Value> = gs
                .live_instances(&reg)
                .into_iter()
                .filter(|e| e.dcc_type != "__gateway__")
                .map(|e| {
                    let name = match e.scene.as_deref() {
                        Some(s) if !s.is_empty() => {
                            format!("{} — {} ({}:{})", e.dcc_type, s, e.host, e.port)
                        }
                        _ => format!("{} @ {}:{}", e.dcc_type, e.host, e.port),
                    };
                    json!({
                        "uri":         format!("dcc://{}/{}", e.dcc_type, e.instance_id),
                        "name":        name,
                        "description": format!("Live {} DCC instance. Version: {}.",
                            e.dcc_type,
                            e.version.as_deref().unwrap_or("unknown")),
                        "mimeType":    "application/json"
                    })
                })
                .collect();
            json!({"jsonrpc":"2.0","id":id,"result":{"resources": resources}})
        }
        "resources/read" => {
            let uri = req
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("");

            // URI format: dcc://{dcc_type}/{instance_id_prefix}
            let parts: Vec<&str> = uri.trim_start_matches("dcc://").splitn(2, '/').collect();

            let reg = gs.registry.read().await;
            let found = gs.live_instances(&reg).into_iter().find(|e| {
                parts.len() == 2
                    && e.dcc_type == parts[0]
                    && e.instance_id.to_string().starts_with(parts[1])
            });

            match found {
                Some(e) => {
                    let detail = entry_to_json(&e, gs.stale_timeout);
                    json!({
                        "jsonrpc": "2.0", "id": id,
                        "result": {
                            "contents": [{
                                "uri":      uri,
                                "mimeType": "application/json",
                                "text":     serde_json::to_string_pretty(&detail)
                                                .unwrap_or_default()
                            }]
                        }
                    })
                }
                None => json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": {"code": -32002, "message": format!("Resource not found: {uri}")}
                }),
            }
        }
        // resources/subscribe — acknowledge; notifications arrive over SSE (GET /mcp).
        "resources/subscribe" | "resources/unsubscribe" => {
            json!({"jsonrpc":"2.0","id":id,"result":{}})
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

            let (text, is_error) = aggregator::route_tools_call(&gs, tool, &args).await;
            json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"content": [{"type": "text", "text": text}], "isError": is_error}
            })
        }
        other => json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        }),
    };

    // Session id: reuse the one the client sent if present; otherwise mint a
    // fresh per-client token so streaming-http clients can correlate their
    // POST requests with their GET /mcp SSE subscription.
    let session_value =
        client_session_id.unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));
    let mut response = Json(resp).into_response();
    if let Ok(hv) = session_value.parse() {
        response.headers_mut().insert("Mcp-Session-Id", hv);
    }
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
