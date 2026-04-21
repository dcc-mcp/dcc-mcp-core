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
/// # Session id
/// The `Mcp-Session-Id` header is honoured: if the client sends one we reuse it;
/// otherwise we mint a fresh token.  The same header is returned in the response
/// so the client can correlate its SSE subscription with later POST /mcp calls.
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

    // Reuse the client's session id if present; otherwise generate one.
    let session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));

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

    let mut resp = Sse::new(endpoint_event.chain(sse_stream))
        .keep_alive(KeepAlive::default())
        .into_response();
    if let Ok(hv) = session_id.parse() {
        resp.headers_mut().insert("Mcp-Session-Id", hv);
    }
    resp
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
///
/// # JSON-RPC batch
/// The body may be a single JSON-RPC object **or** an array of objects (batch).
/// Batches are processed independently; notifications produce no response
/// entries.  If every entry in a batch is a notification an empty `202 Accepted`
/// is returned.
pub async fn handle_gateway_mcp(
    State(gs): State<GatewayState>,
    headers: HeaderMap,
    body: axum::body::Bytes,
) -> Response {
    // Preserve the client's session id (if any) so the SSE subscription opened
    // via GET /mcp can correlate with POST /mcp calls.
    let client_session_id = headers
        .get("Mcp-Session-Id")
        .and_then(|v| v.to_str().ok())
        .map(str::to_owned)
        .unwrap_or_else(|| format!("gw-{}", uuid::Uuid::new_v4().simple()));

    // ── Try single request first, then batch ─────────────────────────────
    let body_val: Value = match serde_json::from_slice(&body) {
        Ok(v) => v,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {e}")}})),
            )
                .into_response();
        }
    };

    if body_val.is_array() {
        let arr = body_val.as_array().unwrap();
        let mut responses: Vec<Value> = Vec::with_capacity(arr.len());
        for item in arr {
            let req = match serde_json::from_value::<JsonRpcRequest>(item.clone()) {
                Ok(r) => r,
                Err(_) => {
                    responses.push(json!({
                        "jsonrpc": "2.0",
                        "id": null,
                        "error": {"code": -32700, "message": "Parse error"}
                    }));
                    continue;
                }
            };
            if req.id.is_none() {
                handle_notification(&gs, &req, &client_session_id).await;
                continue;
            }
            if let Some(resp) = dispatch_single_request(&gs, &req, &client_session_id).await {
                responses.push(resp);
            }
        }
        if responses.is_empty() {
            return StatusCode::ACCEPTED.into_response();
        }
        let mut resp = Json(responses).into_response();
        if let Ok(hv) = client_session_id.parse() {
            resp.headers_mut().insert("Mcp-Session-Id", hv);
        }
        return resp;
    }

    let req = match serde_json::from_value::<JsonRpcRequest>(body_val) {
        Ok(r) => r,
        Err(e) => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({"jsonrpc":"2.0","id":null,"error":{"code":-32700,"message":format!("Parse error: {e}")}})),
            )
                .into_response();
        }
    };

    if req.id.is_none() {
        handle_notification(&gs, &req, &client_session_id).await;
        return StatusCode::ACCEPTED.into_response();
    }

    if let Some(resp) = dispatch_single_request(&gs, &req, &client_session_id).await {
        let mut response = Json(resp).into_response();
        if let Ok(hv) = client_session_id.parse() {
            response.headers_mut().insert("Mcp-Session-Id", hv);
        }
        return response;
    }
    StatusCode::ACCEPTED.into_response()
}

/// Handle an MCP notification (fire-and-forget, no response).
///
/// `notifications/cancelled` is forwarded to the backend that is still
/// processing the corresponding request, if we have it in `pending_calls`.
async fn handle_notification(gs: &GatewayState, req: &JsonRpcRequest, _session_id: &str) {
    match req.method.as_str() {
        "notifications/initialized" => {}
        "notifications/cancelled" => {
            let request_id = req
                .params
                .as_ref()
                .and_then(|p| p.get("requestId"))
                .cloned();
            if let Some(rid) = request_id {
                let rid_str = serde_json::to_string(&rid).unwrap_or_default();
                let pending = gs.pending_calls.read().await;
                if let Some(call) = pending.get(&rid_str) {
                    if !call.backend_url.is_empty() {
                        let cancel_body = json!({
                            "jsonrpc": "2.0",
                            "method": "notifications/cancelled",
                            "params": {"requestId": rid}
                        });
                        let _ = gs
                            .http_client
                            .post(&call.backend_url)
                            .header("content-type", "application/json")
                            .body(cancel_body.to_string())
                            .timeout(std::time::Duration::from_secs(5))
                            .send()
                            .await;
                    }
                }
            }
        }
        other => {
            tracing::debug!(method = other, "Gateway received unknown MCP notification");
        }
    }
}

/// Dispatch one JSON-RPC request (not notification) and return the response
/// value.  Returns `None` when the message is a notification — the caller
/// should translate that into `202 Accepted`.
async fn dispatch_single_request(
    gs: &GatewayState,
    req: &JsonRpcRequest,
    session_id: &str,
) -> Option<Value> {
    let id = req.id.clone()?;
    let id_str = serde_json::to_string(&id).unwrap_or_default();

    match req.method.as_str() {
        "initialize" => {
            // Negotiate protocol version with the client and remember it.
            let client_version = req
                .params
                .as_ref()
                .and_then(|p| p.get("protocolVersion"))
                .and_then(|v| v.as_str());
            let negotiated = negotiate_protocol_version(client_version);
            {
                let mut pv = gs.protocol_version.write().await;
                *pv = Some(negotiated.to_string());
            }

            Some(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {
                    "protocolVersion": negotiated,
                    "capabilities": {
                        "tools": {"listChanged": true},
                        "resources": {"listChanged": true, "subscribe": true}
                    },
                    "serverInfo": {"name": gs.server_name, "version": gs.server_version},
                    "instructions":
                        "DCC-MCP Gateway — unified MCP endpoint across every live DCC.\n\
                         \n\
                         tools/list returns:\n\
                         • 3 discovery meta-tools (list_dcc_instances / get_dcc_instance / connect_to_dcc)\n\
                         • 6 skill-management tools (list/find/search/get_info/load/unload_skill)\n\
                         • Every backend tool, prefixed with an 8-char instance id\n\
                         \n\
                         Workflow:\n\
                         1. search_skills(query=...) — find relevant skills across every live DCC\n\
                         2. load_skill(skill_name=..., instance_id=... when multiple instances exist)\n\
                         3. Call the prefixed tool directly through this same endpoint\n\
                         \n\
                         Subscribe to GET /mcp (SSE) for push notifications."
                }
            }))
        }
        "ping" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "notifications/initialized" => Some(json!({"jsonrpc":"2.0","id":id,"result":{}})),
        "tools/list" => {
            let cursor = req
                .params
                .as_ref()
                .and_then(|p| p.get("cursor"))
                .and_then(|v| v.as_str());
            let result = aggregator::aggregate_tools_list(gs, cursor).await;
            Some(json!({"jsonrpc": "2.0", "id": id, "result": result}))
        }
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
            Some(json!({"jsonrpc":"2.0","id":id,"result":{"resources": resources}}))
        }
        "resources/read" => {
            let uri = req
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("");
            let parts: Vec<&str> = uri.trim_start_matches("dcc://").splitn(2, '/').collect();

            let reg = gs.registry.read().await;
            let found = gs.live_instances(&reg).into_iter().find(|e| {
                parts.len() == 2
                    && e.dcc_type == parts[0]
                    && e.instance_id.to_string().starts_with(parts[1])
            });

            Some(match found {
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
            })
        }
        "resources/subscribe" => {
            let uri = req
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            {
                let mut subs = gs.resource_subscriptions.write().await;
                subs.entry(session_id.to_owned()).or_default().insert(uri);
            }
            Some(json!({"jsonrpc":"2.0","id":id,"result":{}}))
        }
        "resources/unsubscribe" => {
            let uri = req
                .params
                .as_ref()
                .and_then(|p| p.get("uri"))
                .and_then(|v| v.as_str())
                .unwrap_or("")
                .to_owned();
            {
                let mut subs = gs.resource_subscriptions.write().await;
                if let Some(set) = subs.get_mut(session_id) {
                    set.remove(&uri);
                }
            }
            Some(json!({"jsonrpc":"2.0","id":id,"result":{}}))
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
            let meta = req.params.as_ref().and_then(|p| p.get("_meta")).cloned();

            // Register the call so that a later notifications/cancelled can be
            // forwarded to the correct backend.
            {
                let mut pending = gs.pending_calls.write().await;
                pending.insert(
                    id_str.clone(),
                    super::state::PendingCall {
                        backend_url: String::new(), // filled after routing
                        backend_request_id: id_str.clone(),
                    },
                );
            }

            let (text, is_error) =
                aggregator::route_tools_call(gs, tool, &args, meta.as_ref(), Some(id_str.clone()))
                    .await;

            {
                let mut pending = gs.pending_calls.write().await;
                pending.remove(&id_str);
            }

            Some(json!({
                "jsonrpc": "2.0", "id": id,
                "result": {"content": [{"type": "text", "text": text}], "isError": is_error}
            }))
        }
        other => Some(json!({
            "jsonrpc": "2.0", "id": id,
            "error": {"code": -32601, "message": format!("Method not found: {other}")}
        })),
    }
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
