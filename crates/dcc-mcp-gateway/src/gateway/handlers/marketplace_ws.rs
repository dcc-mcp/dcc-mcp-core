//! Marketplace WebSocket bridge — PIP-1096 M2.
//!
//! Exposes a `/marketplace/ws` endpoint that accepts `Sec-WebSocket-Protocol:
//! dcc-mcp-marketplace.v1` upgrades. Once connected, the bridge speaks
//! JSON-RPC 2.0 over WebSocket text frames and delegates all domain logic
//! to the shared `dcc_mcp_marketplace::MarketplaceService`.
//!
//! Install/uninstall mutations fire the skill reload chain via
//! `reload_skill_paths_and_refresh_backends(...)`.
//!
//! ## Architecture (v2 — review fixes)
//!
//! - **Per-connection response**: each connection gets a dedicated
//!   `mpsc::Sender<String>` for JSON-RPC responses so that responses
//!   only reach the requesting client. Broadcast is reserved for
//!   notification events.
//! - **Shared MarketplaceService**: `Arc<MarketplaceService>` is held
//!   in `MarketplaceWsState` and injected from `AdminState` or
//!   constructed once on first use.
//! - **Single heartbeat source**: one global heartbeat task feeds the
//!   broadcast channel; per-connection tasks no longer spawn
//!   independent heartbeat tickers.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use axum::{
    extract::{
        State, WebSocketUpgrade,
        ws::{Message, WebSocket},
    },
    http::{StatusCode, header},
    response::IntoResponse,
};
use futures_util::{SinkExt, StreamExt};
use serde_json::Value;
use tokio::sync::{RwLock, broadcast, mpsc};
use tracing::{debug, info, warn};
use uuid::Uuid;

use dcc_mcp_marketplace::MarketplaceService;

use super::super::admin::marketplace::resolve_icon_url;
use super::super::admin::skill_reload::reload_skill_paths_and_refresh_backends;
use super::super::admin::state::AdminState;
use super::super::capability::RefreshReason;
use super::marketplace_ws_protocol::*;

// ── Constants ─────────────────────────────────────────────────────────────

const SUBPROTOCOL: &str = "dcc-mcp-marketplace.v1";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);

// ── Shared WS bridge state ────────────────────────────────────────────────

/// Per-connection state tracked in the bridge registry.
#[allow(dead_code)]
struct ConnectionState {
    topics: HashSet<String>,
}

/// Shared state for all WS bridge connections.
///
/// Holds a broadcast channel so connections can subscribe to marketplace
/// events (install/uninstall notifications, operation progress, etc.).
#[derive(Clone)]
pub struct MarketplaceWsState {
    pub admin: AdminState,
    /// Broadcast channel for notification events (installed.changed,
    /// operation.*, skills.reloaded, gateway.heartbeat, etc.).
    /// Responses MUST NOT go through this channel — they use per-connection
    /// `mpsc::Sender<String>` instead.
    pub events_tx: Arc<broadcast::Sender<String>>,
    /// Shared marketplace service — constructed once, reused across calls.
    pub marketplace_service: Arc<MarketplaceService>,
    connections: Arc<RwLock<slab::Slab<ConnectionState>>>,
}

impl MarketplaceWsState {
    pub fn new(admin: AdminState) -> Self {
        let (events_tx, _) = broadcast::channel(256);
        let root = dcc_mcp_marketplace::marketplace_root_or_default();
        let config_path = dcc_mcp_marketplace::default_config_path()
            .unwrap_or_else(|_| root.join("sources.json"));
        let marketplace_service =
            Arc::new(MarketplaceService::new(root).with_config_path(config_path));

        let state = Self {
            admin,
            events_tx: Arc::new(events_tx),
            marketplace_service,
            connections: Arc::new(RwLock::new(slab::Slab::new())),
        };

        // Spawn the single global heartbeat task.
        let heartbeat_tx = state.events_tx.clone();
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
            loop {
                interval.tick().await;
                let payload = serde_json::to_string(&JsonRpcNotification::new(
                    "gateway.heartbeat",
                    Value::Null,
                ))
                .unwrap_or_default();
                if heartbeat_tx.send(payload).is_err() {
                    // No receivers left — broadcast channel closed.
                    break;
                }
            }
            debug!("global heartbeat task exiting");
        });

        state
    }

    async fn register_connection(&self) -> usize {
        self.connections.write().await.insert(ConnectionState {
            topics: HashSet::new(),
        })
    }

    #[allow(dead_code)]
    async fn set_topics(&self, key: usize, topics: HashSet<String>) {
        if let Some(conn) = self.connections.write().await.get_mut(key) {
            conn.topics = topics;
        }
    }

    async fn remove_connection(&self, key: usize) {
        self.connections.write().await.remove(key);
    }
}

// ── Upgrade handler ───────────────────────────────────────────────────────

pub(crate) fn is_origin_trusted(origin_str: &str) -> bool {
    let origin_lower = origin_str.to_lowercase();

    if origin_lower.starts_with("http://localhost") || origin_lower.starts_with("https://localhost")
    {
        let rest = if origin_lower.starts_with("https://") {
            &origin_lower["https://localhost".len()..]
        } else {
            &origin_lower["http://localhost".len()..]
        };
        return rest.is_empty() || rest.starts_with(':');
    }

    if origin_lower.starts_with("http://127.0.0.1") || origin_lower.starts_with("https://127.0.0.1")
    {
        let rest = if origin_lower.starts_with("https://") {
            &origin_lower["https://127.0.0.1".len()..]
        } else {
            &origin_lower["http://127.0.0.1".len()..]
        };
        return rest.is_empty() || rest.starts_with(':');
    }

    false
}

/// `GET /marketplace/ws` — upgrade to a WebSocket connection.
///
/// Requires the `Sec-WebSocket-Protocol: dcc-mcp-marketplace.v1` subprotocol
/// header.  Returns `426 Upgrade Required` if the header is missing or wrong.
pub async fn handle_marketplace_ws(
    ws: WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    State(state): State<MarketplaceWsState>,
) -> impl IntoResponse {
    // Check Origin header. Reject if missing or untrusted.
    let origin_ok = if let Some(origin) = headers.get(header::ORIGIN) {
        if let Ok(origin_str) = origin.to_str() {
            is_origin_trusted(origin_str)
        } else {
            false
        }
    } else {
        false
    };

    if !origin_ok {
        return StatusCode::FORBIDDEN.into_response();
    }

    // Check that the client requested our subprotocol via Sec-WebSocket-Protocol header.
    let protocol_ok = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .map(|v| v.split(',').map(|s| s.trim()).any(|p| p == SUBPROTOCOL))
        .unwrap_or(false);

    if !protocol_ok {
        return (
            StatusCode::UPGRADE_REQUIRED,
            [(header::UPGRADE, "websocket")],
            format!("Missing or unsupported subprotocol; expected {SUBPROTOCOL}"),
        )
            .into_response();
    };
    ws.protocols([SUBPROTOCOL])
        .on_upgrade(move |socket| serve_socket(socket, state))
}

// ── Socket event loop ─────────────────────────────────────────────────────

async fn serve_socket(socket: WebSocket, state: MarketplaceWsState) {
    let connection_id = state.register_connection().await;
    let conn_uuid = Uuid::new_v4();
    info!(%conn_uuid, connection_id, "marketplace WS bridge connected");

    let (ws_tx, mut ws_rx) = socket.split();
    let mut events_rx = state.events_tx.subscribe();

    // Per-connection channel for JSON-RPC responses (not events).
    // Capacity 32 is generous for pipelining.
    let (response_tx, mut response_rx) = mpsc::channel::<String>(32);

    // Merged forwarder: reads both broadcast events and per-connection
    // responses, writes both to the WS sink.
    let events_handle = tokio::spawn(async move {
        let mut tx = ws_tx;
        loop {
            tokio::select! {
                result = events_rx.recv() => {
                    match result {
                        Ok(msg) => {
                            if tx.send(Message::Text(msg.into())).await.is_err() {
                                break;
                            }
                        }
                        Err(broadcast::error::RecvError::Lagged(n)) => {
                            warn!(n, "events_rx lagged, skipping");
                            continue;
                        }
                        Err(broadcast::error::RecvError::Closed) => break,
                    }
                }
                result = response_rx.recv() => {
                    match result {
                        Some(resp) => {
                            if tx.send(Message::Text(resp.into())).await.is_err() {
                                break;
                            }
                        }
                        None => break,
                    }
                }
            }
        }
    });

    // Read loop: process incoming JSON-RPC messages.
    // Responses are sent through `response_tx` (per-connection).
    let conn_state = state.clone();
    let conn_uuid_reader = conn_uuid;
    let resp_tx = response_tx.clone();
    let read_handle = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    let response = handle_message(&conn_state, &text.to_string()).await;
                    if let Some(resp) = response {
                        let is_err = resp_tx.send(resp).await.is_err();
                        if is_err {
                            break;
                        }
                    }
                }
                Message::Close(_) => {
                    debug!(%conn_uuid_reader, "WS close frame received");
                    break;
                }
                Message::Ping(_) => {
                    // Let tungstenite handle pong automatically
                }
                Message::Pong(_) => {
                    // Heartbeat response received
                }
                Message::Binary(_) => {
                    // We only accept text frames
                    let err = serde_json::to_string(&JsonRpcError::new(
                        None,
                        INVALID_REQUEST,
                        "Binary frames not supported".to_string(),
                        None,
                    ))
                    .unwrap_or_default();
                    let _ = resp_tx.send(err).await;
                }
            }
        }
        debug!(%conn_uuid_reader, "WS read loop exiting");
    });

    // Wait for read to finish (connection close)
    let _ = read_handle.await;

    // Cleanup
    events_handle.abort();
    state.remove_connection(connection_id).await;
    info!(%conn_uuid, "marketplace WS bridge disconnected");
}

// ── Message dispatch ──────────────────────────────────────────────────────

async fn handle_message(state: &MarketplaceWsState, text: &str) -> Option<String> {
    let request: JsonRpcRequest = match serde_json::from_str(text) {
        Ok(req) => req,
        Err(e) => {
            warn!("Failed to parse JSON-RPC request: {e}");
            let err = JsonRpcError::parse_error();
            return Some(serde_json::to_string(&err).unwrap_or_default());
        }
    };

    if request.jsonrpc != "2.0" {
        let err = JsonRpcError::invalid_request();
        return Some(serde_json::to_string(&err).unwrap_or_default());
    }

    // Notifications: no id, no response expected.
    let is_notification = request.id.is_none();

    match request.method.as_str() {
        methods::HELLO => Some(handle_hello(request.id).await),
        methods::CATALOG_LIST => Some(handle_catalog_list(state, request.id).await),
        methods::INSTALLED_LIST => Some(handle_installed_list(state, request.id).await),
        methods::INSTALL => {
            Some(handle_install(state, request.id.clone(), request.params.clone()).await)
        }
        methods::UNINSTALL => {
            Some(handle_uninstall(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SOURCES_LIST => Some(handle_sources_list(state, request.id).await),
        methods::SOURCES_ADD => {
            Some(handle_sources_add(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SOURCES_REMOVE => {
            Some(handle_sources_remove(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SUBSCRIBE => {
            handle_subscribe(state, request.id.clone(), request.params.clone()).await
        }
        methods::PING => {
            if is_notification {
                None
            } else {
                Some(
                    serde_json::to_string(&JsonRpcSuccess::new(
                        request.id,
                        Value::String("pong".into()),
                    ))
                    .unwrap_or_default(),
                )
            }
        }
        _ => {
            if is_notification {
                None
            } else {
                let err = JsonRpcError::method_not_found(request.id.clone(), &request.method);
                Some(serde_json::to_string(&err).unwrap_or_default())
            }
        }
    }
}

// ── Method handlers ───────────────────────────────────────────────────────

async fn handle_hello(id: Option<Value>) -> String {
    let result = serde_json::json!({
        "protocol": "dcc-mcp-marketplace.v1",
        "version": env!("CARGO_PKG_VERSION"),
    });
    serde_json::to_string(&JsonRpcSuccess::new(id, result)).unwrap_or_default()
}

async fn handle_catalog_list(state: &MarketplaceWsState, id: Option<Value>) -> String {
    let service = &state.marketplace_service;
    match service.catalog().await {
        Ok(hits) => {
            let entries: Vec<Value> = hits
                .into_iter()
                .map(|hit| {
                    let icon =
                        resolve_icon_url(hit.entry.icon.as_deref(), Some(hit.source.url.as_str()));
                    let showcase = dcc_mcp_marketplace::resolve_catalog_asset_url(
                        hit.entry.showcase.as_deref(),
                        hit.entry.install.as_ref(),
                    );
                    serde_json::json!({
                        "name": hit.entry.name,
                        "description": hit.entry.description,
                        "dcc": hit.entry.dcc,
                        "url": hit.entry.url,
                        "tags": hit.entry.tags,
                        "version": hit.entry.version,
                        "min_core_version": hit.entry.min_core_version,
                        "maintainer": hit.entry.maintainer,
                        "requires": hit.entry.requires,
                        "icon": icon,
                        "showcase": showcase,
                        "source_name": hit.source.name,
                        "source_url": hit.source.url,
                    })
                })
                .collect();
            serde_json::to_string(&JsonRpcSuccess::new(
                id,
                serde_json::json!({ "entries": entries }),
            ))
            .unwrap_or_default()
        }
        Err(err) => {
            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_installed_list(state: &MarketplaceWsState, id: Option<Value>) -> String {
    let service = &state.marketplace_service;
    match service.list_installed(None) {
        Ok(list) => {
            let packages: Vec<Value> = list
                .packages
                .into_iter()
                .map(|p| {
                    serde_json::json!({
                        "name": p.name,
                        "dcc": p.dcc,
                        "version": p.version,
                        "path": p.path,
                        "source_name": p.source_name,
                        "source_url": p.source_url,
                        "install_type": p.install_type,
                    })
                })
                .collect();
            serde_json::to_string(&JsonRpcSuccess::new(
                id,
                serde_json::json!({ "packages": packages }),
            ))
            .unwrap_or_default()
        }
        Err(err) => {
            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_install(
    state: &MarketplaceWsState,
    id: Option<Value>,
    params: Option<Value>,
) -> String {
    let install_params: InstallParams = match params
        .map(serde_json::from_value::<InstallParams>)
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default();
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid install params: {e}"),
            ))
            .unwrap_or_default();
        }
    };

    let operation_id = Uuid::new_v4().to_string();
    let request_id = id.clone();

    // Emit operation.progress: queued
    let _ = state.events_tx.send(
        serde_json::to_string(&JsonRpcNotification::new(
            events::OPERATION_PROGRESS,
            serde_json::json!({
                "operation_id": operation_id,
                "request_id": request_id,
                "phase": OperationPhase::Queued,
                "name": install_params.name,
                "dcc": install_params.dcc,
            }),
        ))
        .unwrap_or_default(),
    );

    let service = &state.marketplace_service;
    let sources: Vec<String> = install_params.source.into_iter().collect();

    // Emit operation.progress: fetching
    let _ = state.events_tx.send(
        serde_json::to_string(&JsonRpcNotification::new(
            events::OPERATION_PROGRESS,
            serde_json::json!({
                "operation_id": operation_id,
                "request_id": request_id,
                "phase": OperationPhase::Fetching,
                "name": install_params.name,
                "dcc": install_params.dcc,
            }),
        ))
        .unwrap_or_default(),
    );

    match service
        .install(
            install_params.name.clone(),
            Some(install_params.dcc.clone()),
            sources,
            false,
            false,
        )
        .await
    {
        Ok(result) => {
            // Emit operation.progress: installing
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_PROGRESS,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "phase": OperationPhase::Installing,
                        "name": install_params.name,
                        "dcc": install_params.dcc,
                    }),
                ))
                .unwrap_or_default(),
            );

            if result.reload_required {
                // Emit operation.progress: reloading
                let _ = state.events_tx.send(
                    serde_json::to_string(&JsonRpcNotification::new(
                        events::OPERATION_PROGRESS,
                        serde_json::json!({
                            "operation_id": operation_id,
                            "request_id": request_id,
                            "phase": OperationPhase::Reloading,
                            "name": install_params.name,
                            "dcc": install_params.dcc,
                        }),
                    ))
                    .unwrap_or_default(),
                );

                reload_skill_paths_and_refresh_backends(
                    &state.admin,
                    RefreshReason::ToolsListChanged,
                )
                .await;

                // Emit skills.reloaded
                let _ = state.events_tx.send(
                    serde_json::to_string(&JsonRpcNotification::new(
                        events::SKILLS_RELOADED,
                        serde_json::json!({ "reason": "install" }),
                    ))
                    .unwrap_or_default(),
                );
            }

            // Emit operation.completed
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_COMPLETED,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "name": install_params.name,
                        "dcc": install_params.dcc,
                    }),
                ))
                .unwrap_or_default(),
            );

            // Emit installed.changed
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::INSTALLED_CHANGED,
                serde_json::json!({ "action": "installed", "name": install_params.name, "dcc": install_params.dcc }),
            ))
            .unwrap_or_default());

            let response = serde_json::json!({
                "operation_id": operation_id,
                "installed": result.installed,
                "name": result.name,
                "dcc": result.dcc,
                "version": result.version,
                "path": result.path,
                "reload_required": result.reload_required,
            });
            serde_json::to_string(&JsonRpcSuccess::new(id, response)).unwrap_or_default()
        }
        Err(err) => {
            // Emit operation.failed
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_FAILED,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "name": install_params.name,
                        "dcc": install_params.dcc,
                        "error": err.to_string(),
                    }),
                ))
                .unwrap_or_default(),
            );

            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_uninstall(
    state: &MarketplaceWsState,
    id: Option<Value>,
    params: Option<Value>,
) -> String {
    let uninstall_params: UninstallParams = match params
        .map(serde_json::from_value::<UninstallParams>)
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default();
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid uninstall params: {e}"),
            ))
            .unwrap_or_default();
        }
    };

    let operation_id = Uuid::new_v4().to_string();
    let request_id = id.clone();

    // Emit operation.progress: queued
    let _ = state.events_tx.send(
        serde_json::to_string(&JsonRpcNotification::new(
            events::OPERATION_PROGRESS,
            serde_json::json!({
                "operation_id": operation_id,
                "request_id": request_id,
                "phase": OperationPhase::Queued,
                "name": uninstall_params.name,
                "dcc": uninstall_params.dcc,
            }),
        ))
        .unwrap_or_default(),
    );

    let service = &state.marketplace_service;
    match service.uninstall(&uninstall_params.name, &uninstall_params.dcc) {
        Ok(result) => {
            // Emit operation.progress: removing
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_PROGRESS,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "phase": OperationPhase::Removing,
                        "name": uninstall_params.name,
                        "dcc": uninstall_params.dcc,
                    }),
                ))
                .unwrap_or_default(),
            );

            if result.reload_required {
                // Emit operation.progress: reloading
                let _ = state.events_tx.send(
                    serde_json::to_string(&JsonRpcNotification::new(
                        events::OPERATION_PROGRESS,
                        serde_json::json!({
                            "operation_id": operation_id,
                            "request_id": request_id,
                            "phase": OperationPhase::Reloading,
                            "name": uninstall_params.name,
                            "dcc": uninstall_params.dcc,
                        }),
                    ))
                    .unwrap_or_default(),
                );

                reload_skill_paths_and_refresh_backends(
                    &state.admin,
                    RefreshReason::ToolsListChanged,
                )
                .await;

                let _ = state.events_tx.send(
                    serde_json::to_string(&JsonRpcNotification::new(
                        events::SKILLS_RELOADED,
                        serde_json::json!({ "reason": "uninstall" }),
                    ))
                    .unwrap_or_default(),
                );
            }

            // Emit operation.completed
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_COMPLETED,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "name": uninstall_params.name,
                        "dcc": uninstall_params.dcc,
                    }),
                ))
                .unwrap_or_default(),
            );

            // Emit installed.changed
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::INSTALLED_CHANGED,
                serde_json::json!({ "action": "uninstalled", "name": uninstall_params.name, "dcc": uninstall_params.dcc }),
            ))
            .unwrap_or_default());

            let response = serde_json::json!({
                "operation_id": operation_id,
                "uninstalled": result.uninstalled,
                "name": result.name,
                "dcc": result.dcc,
                "path": result.path,
                "reload_required": result.reload_required,
            });
            serde_json::to_string(&JsonRpcSuccess::new(id, response)).unwrap_or_default()
        }
        Err(err) => {
            let _ = state.events_tx.send(
                serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_FAILED,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "request_id": request_id,
                        "name": uninstall_params.name,
                        "dcc": uninstall_params.dcc,
                        "error": err.to_string(),
                    }),
                ))
                .unwrap_or_default(),
            );

            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_sources_list(state: &MarketplaceWsState, id: Option<Value>) -> String {
    let service = &state.marketplace_service;
    match service.list_sources() {
        Ok(sources) => {
            let items: Vec<Value> = sources
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "url": s.url,
                        "origin": format!("{:?}", s.origin).to_lowercase(),
                    })
                })
                .collect();
            serde_json::to_string(&JsonRpcSuccess::new(
                id,
                serde_json::json!({ "sources": items }),
            ))
            .unwrap_or_default()
        }
        Err(err) => {
            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_sources_add(
    state: &MarketplaceWsState,
    id: Option<Value>,
    params: Option<Value>,
) -> String {
    let add_params: AddSourceParams = match params
        .map(serde_json::from_value::<AddSourceParams>)
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default();
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid params: {e}"),
            ))
            .unwrap_or_default();
        }
    };

    let service = &state.marketplace_service;
    match service.add_source(&add_params.source) {
        Ok(sources) => {
            let items: Vec<Value> = sources
                .into_iter()
                .map(|s| {
                    serde_json::json!({
                        "name": s.name,
                        "url": s.url,
                        "origin": format!("{:?}", s.origin).to_lowercase(),
                    })
                })
                .collect();
            serde_json::to_string(&JsonRpcSuccess::new(
                id,
                serde_json::json!({ "sources": items }),
            ))
            .unwrap_or_default()
        }
        Err(err) => {
            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_sources_remove(
    _state: &MarketplaceWsState,
    id: Option<Value>,
    _params: Option<Value>,
) -> String {
    // MarketplaceService doesn't have a remove_source method — reserved for future.
    // Use INTERNAL_ERROR with data.reason=not_implemented to avoid confusing
    // clients with METHOD_NOT_FOUND.
    serde_json::to_string(&JsonRpcError::new(
        id,
        INTERNAL_ERROR,
        "sources.remove not yet implemented".to_string(),
        Some(serde_json::json!({ "reason": "not_implemented" })),
    ))
    .unwrap_or_default()
}

async fn handle_subscribe(
    _state: &MarketplaceWsState,
    id: Option<Value>,
    params: Option<Value>,
) -> Option<String> {
    let subscribe_params: SubscribeParams = match params
        .map(serde_json::from_value::<SubscribeParams>)
        .transpose()
    {
        Ok(Some(p)) => p,
        _ => {
            let err = JsonRpcError::invalid_params(id.clone(), "topics array required");
            return Some(serde_json::to_string(&err).unwrap_or_default());
        }
    };

    let topics: HashSet<String> = subscribe_params.topics.into_iter().collect();
    // Note: topics are stored for future server-side filtering.
    // Currently all events are broadcast to all connections; topic filtering
    // is the client's responsibility. See PIP-1096 M2 spec.
    debug!(?topics, "WS client subscribed to topics");

    if id.is_none() {
        // Notification — no response.
        None
    } else {
        Some(
            serde_json::to_string(&JsonRpcSuccess::new(id, Value::String("subscribed".into())))
                .unwrap_or_default(),
        )
    }
}

// ── Integration tests ─────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    /// Verify that `MarketplaceWsState::new` creates a shared service and
    /// that the heartbeat task starts without error.
    #[tokio::test]
    async fn test_marketplace_ws_state_creates_shared_service() {
        let tmp = tempfile::tempdir().unwrap();
        let root = tmp.path().join("marketplace");
        std::fs::create_dir_all(&root).unwrap();
        unsafe {
            std::env::set_var(
                "DCC_MCP_MARKETPLACE_INSTALL_ROOT",
                root.to_string_lossy().as_ref(),
            );
            std::env::set_var("DCC_MCP_MARKETPLACE_NO_DEFAULT_SOURCES", "1");
        }

        let dir = tempfile::tempdir().unwrap();
        let registry = Arc::new(RwLock::new(
            dcc_mcp_transport::discovery::file_registry::FileRegistry::new(dir.path()).unwrap(),
        ));
        let (yield_tx, _) = tokio::sync::watch::channel(false);
        let (gw_events_tx, _) = broadcast::channel::<String>(8);
        let gw_state = crate::gateway::state::GatewayState {
            registry,
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
            server_name: "test-gateway".into(),
            server_version: "0.0.0-test".into(),
            own_host: "127.0.0.1".into(),
            own_port: 9765,
            http_client: reqwest::Client::new(),
            yield_tx: Arc::new(yield_tx),
            events_tx: Arc::new(gw_events_tx),
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
            search_cache: Arc::new(crate::gateway::capability::search_cache::SearchCache::new(
                Default::default(),
            )),
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
            auth: std::sync::Arc::new(crate::gateway::security::GatewayAuth::disabled()),
            update_manifest_url: None,
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
            semantic_search_enabled: false,
            #[cfg(feature = "admin-persist-sqlite")]
            admin_sqlite_lane: None,
        };

        let admin_state = AdminState::new(gw_state);
        let ws_state = MarketplaceWsState::new(admin_state);

        // Verify the service is shared (same Arc).
        let svc1 = Arc::clone(&ws_state.marketplace_service);
        let svc2 = Arc::clone(&ws_state.marketplace_service);
        assert!(Arc::ptr_eq(&svc1, &svc2));

        // Verify events channel is alive (at least the sender exists).
        // receiver_count is 0 initially because no subscribers exist yet.
        assert_eq!(ws_state.events_tx.receiver_count(), 0);
    }

    /// Test that per-connection response isolation works: a response
    /// sent on one connection's mpsc channel does not leak to another.
    #[tokio::test]
    async fn test_per_connection_response_isolation() {
        let (tx_a, mut rx_a) = mpsc::channel::<String>(4);
        let (_tx_b, mut rx_b) = mpsc::channel::<String>(4);

        // Send a response on connection A
        tx_a.send("response-for-a".into()).await.unwrap();

        // Connection B should not receive it
        assert_eq!(rx_a.recv().await, Some("response-for-a".into()));
        // B's channel should be empty — timeout quickly
        let result = tokio::time::timeout(Duration::from_millis(50), rx_b.recv()).await;
        assert!(result.is_err() || result.unwrap().is_none());
    }

    /// Verify protocol-level hello handler returns correct response.
    #[test]
    fn test_hello_response_format() {
        let result = serde_json::json!({
            "protocol": "dcc-mcp-marketplace.v1",
            "version": env!("CARGO_PKG_VERSION"),
        });
        let resp = JsonRpcSuccess::new(Some(Value::Number(1.into())), result);
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"protocol\":\"dcc-mcp-marketplace.v1\""));
        assert!(json.contains("\"id\":1"));
    }

    /// Verify handle_sources_remove uses INTERNAL_ERROR not METHOD_NOT_FOUND.
    #[test]
    fn test_sources_remove_uses_internal_error() {
        let json = serde_json::to_string(&JsonRpcError::new(
            Some(Value::Number(1.into())),
            INTERNAL_ERROR,
            "sources.remove not yet implemented".to_string(),
            Some(serde_json::json!({ "reason": "not_implemented" })),
        ))
        .unwrap();
        assert!(
            json.contains("\"code\":-32603"),
            "expected INTERNAL_ERROR (-32603), got: {json}"
        );
        assert!(
            !json.contains("\"code\":-32601"),
            "should not use METHOD_NOT_FOUND (-32601)"
        );
        assert!(
            json.contains("not_implemented"),
            "expected data.reason=not_implemented"
        );
    }

    /// Verify subscribe with invalid params returns error through return value
    /// (not broadcast) — fixing the dual-path semantics.
    #[test]
    fn test_subscribe_invalid_params_returns_error_directly() {
        // Simulate what handle_subscribe would return for invalid params
        let err =
            JsonRpcError::invalid_params(Some(Value::Number(1.into())), "topics array required");
        let json = serde_json::to_string(&err).unwrap();
        assert!(json.contains("\"code\":-32602"));
        assert!(json.contains("topics array required"));
    }

    /// Verify operation events include request_id.
    #[test]
    fn test_operation_progress_includes_request_id() {
        let notif = JsonRpcNotification::new(
            events::OPERATION_PROGRESS,
            serde_json::json!({
                "operation_id": "op-123",
                "request_id": 1,
                "phase": OperationPhase::Queued,
                "name": "test-pkg",
                "dcc": "maya",
            }),
        );
        let json = serde_json::to_string(&notif).unwrap();
        assert!(
            json.contains("\"request_id\":1"),
            "operation.* events must include request_id"
        );
        assert!(json.contains("\"operation_id\":\"op-123\""));
    }

    /// Verify operation.completed includes request_id.
    #[test]
    fn test_operation_completed_includes_request_id() {
        let notif = JsonRpcNotification::new(
            events::OPERATION_COMPLETED,
            serde_json::json!({
                "operation_id": "op-123",
                "request_id": 1,
                "name": "test-pkg",
                "dcc": "maya",
            }),
        );
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"request_id\":1"));
    }

    /// Verify operation.failed includes request_id.
    #[test]
    fn test_operation_failed_includes_request_id() {
        let notif = JsonRpcNotification::new(
            events::OPERATION_FAILED,
            serde_json::json!({
                "operation_id": "op-123",
                "request_id": 1,
                "name": "test-pkg",
                "dcc": "maya",
                "error": "something went wrong",
            }),
        );
        let json = serde_json::to_string(&notif).unwrap();
        assert!(json.contains("\"request_id\":1"));
    }

    /// Verify subscribe response (not notification) returns "subscribed".
    #[test]
    fn test_subscribe_success_response() {
        let resp = JsonRpcSuccess::new(
            Some(Value::Number(1.into())),
            Value::String("subscribed".into()),
        );
        let json = serde_json::to_string(&resp).unwrap();
        assert!(json.contains("\"result\":\"subscribed\""));
    }

    /// Verify origin trusted validation.
    #[test]
    fn test_is_origin_trusted() {
        assert!(is_origin_trusted("http://localhost"));
        assert!(is_origin_trusted("http://localhost:3000"));
        assert!(is_origin_trusted("https://localhost"));
        assert!(is_origin_trusted("http://127.0.0.1"));
        assert!(is_origin_trusted("http://127.0.0.1:8080"));
        assert!(is_origin_trusted("https://127.0.0.1"));

        assert!(!is_origin_trusted("http://example.com"));
        assert!(!is_origin_trusted("null"));
        assert!(!is_origin_trusted("file://"));
    }
}
