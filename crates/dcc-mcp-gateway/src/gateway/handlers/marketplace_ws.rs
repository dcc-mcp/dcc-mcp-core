//! Marketplace WebSocket bridge — PIP-1096 M2.
//!
//! Exposes a `/marketplace/ws` endpoint that accepts `Sec-WebSocket-Protocol:
//! dcc-mcp-marketplace.v1` upgrades. Once connected, the bridge speaks
//! JSON-RPC 2.0 over WebSocket text frames and delegates all domain logic
//! to the shared `dcc_mcp_marketplace::MarketplaceService`.
//!
//! Install/uninstall mutations fire the skill reload chain via
//! `reload_skill_paths_and_refresh_backends(...)`.

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
use tokio::sync::{broadcast, RwLock};
use tracing::{debug, info, warn};
use uuid::Uuid;

use dcc_mcp_marketplace::MarketplaceService;

use super::super::admin::skill_reload::reload_skill_paths_and_refresh_backends;
use super::super::admin::state::AdminState;
use super::super::capability::RefreshReason;
use super::marketplace_ws_protocol::*;

// ── Constants ─────────────────────────────────────────────────────────────

const SUBPROTOCOL: &str = "dcc-mcp-marketplace.v1";
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(15);
#[allow(dead_code)]
const HEARTBEAT_TIMEOUT: Duration = Duration::from_secs(45);

// ── Shared WS bridge state ────────────────────────────────────────────────

/// Per-connection state tracked in the bridge registry.
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
    pub events_tx: Arc<broadcast::Sender<String>>,
    connections: Arc<RwLock<slab::Slab<ConnectionState>>>,
}

impl MarketplaceWsState {
    pub fn new(admin: AdminState) -> Self {
        let (events_tx, _) = broadcast::channel(256);
        Self {
            admin,
            events_tx: Arc::new(events_tx),
            connections: Arc::new(RwLock::new(slab::Slab::new())),
        }
    }

    async fn register_connection(&self) -> usize {
        self.connections
            .write()
            .await
            .insert(ConnectionState {
                topics: HashSet::new(),
            })
    }

    async fn set_topics(&self, key: usize, topics: HashSet<String>) {
        if let Some(conn) = self.connections.write().await.get_mut(key) {
            conn.topics = topics;
        }
    }

    async fn remove_connection(&self, key: usize) {
        self.connections.write().await.remove(key);
    }

    fn marketplace_service(&self) -> MarketplaceService {
        let root = dcc_mcp_marketplace::marketplace_root_or_default();
        let config_path = dcc_mcp_marketplace::default_config_path()
            .unwrap_or_else(|_| root.join("sources.json"));
        MarketplaceService::new(root).with_config_path(config_path)
    }
}

// ── Upgrade handler ───────────────────────────────────────────────────────

/// `GET /marketplace/ws` — upgrade to a WebSocket connection.
///
/// Requires the `Sec-WebSocket-Protocol: dcc-mcp-marketplace.v1` subprotocol
/// header.  Returns `426 Upgrade Required` if the header is missing or wrong.
pub async fn handle_marketplace_ws(
    ws: WebSocketUpgrade,
    headers: axum::http::HeaderMap,
    State(state): State<MarketplaceWsState>,
) -> impl IntoResponse {
    // Check that the client requested our subprotocol via Sec-WebSocket-Protocol header.
    let protocol_ok = headers
        .get(header::SEC_WEBSOCKET_PROTOCOL)
        .and_then(|v| v.to_str().ok())
        .map(|v| {
            v.split(',')
                .map(|s| s.trim())
                .any(|p| p == SUBPROTOCOL)
        })
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

    let (mut ws_tx, mut ws_rx) = socket.split();
    let mut events_rx = state.events_tx.subscribe();

    // Spawn heartbeat ticker
    let heartbeat_tx = state.events_tx.clone();
    let conn_uuid_hb = conn_uuid;
    let heartbeat_handle = tokio::spawn(async move {
        let mut interval = tokio::time::interval(HEARTBEAT_INTERVAL);
        loop {
            interval.tick().await;
            let payload = serde_json::to_string(&JsonRpcNotification::new(
                "gateway.heartbeat",
                Value::Null,
            ))
            .unwrap_or_default();
            if heartbeat_tx.send(payload).is_err() {
                break;
            }
        }
        debug!(%conn_uuid_hb, "heartbeat task exiting");
    });

    // Event forwarder: reads from the broadcast channel and writes to WS.
    let events_handle = {
        let mut tx = ws_tx;
        tokio::spawn(async move {
            while let Ok(msg) = events_rx.recv().await {
                if tx
                    .send(Message::Text(msg.into()))
                    .await
                    .is_err()
                {
                    break;
                }
            }
        })
    };

    // Read loop: process incoming JSON-RPC messages.
    let conn_state = state.clone();
    let conn_uuid_reader = conn_uuid;
    let read_handle = tokio::spawn(async move {
        while let Some(Ok(msg)) = ws_rx.next().await {
            match msg {
                Message::Text(text) => {
                    let response = handle_message(&conn_state, &text.to_string()).await;
                    // Send response back to events channel
                    if let Some(resp) = response {
                        let _ = conn_state.events_tx.send(resp);
                    }
                }
                Message::Close(_) => {
                    debug!(%conn_uuid_reader, "WS close frame received");
                    break;
                }
                Message::Ping(data) => {
                    // Let tungstenite handle pong automatically
                    let _ = data;
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
                    let _ = conn_state.events_tx.send(err);
                }
            }
        }
        debug!(%conn_uuid_reader, "WS read loop exiting");
    });

    // Wait for read to finish (connection close)
    let _ = read_handle.await;

    // Cleanup
    heartbeat_handle.abort();
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
        methods::HELLO => {
            Some(handle_hello(request.id.clone()).await)
        }
        methods::CATALOG_LIST => {
            Some(handle_catalog_list(state, request.id.clone()).await)
        }
        methods::INSTALLED_LIST => {
            Some(handle_installed_list(state, request.id.clone()).await)
        }
        methods::INSTALL => {
            Some(handle_install(state, request.id.clone(), request.params.clone()).await)
        }
        methods::UNINSTALL => {
            Some(handle_uninstall(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SOURCES_LIST => {
            Some(handle_sources_list(state, request.id.clone()).await)
        }
        methods::SOURCES_ADD => {
            Some(handle_sources_add(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SOURCES_REMOVE => {
            Some(handle_sources_remove(state, request.id.clone(), request.params.clone()).await)
        }
        methods::SUBSCRIBE => {
            handle_subscribe(state, request.id.clone(), request.params.clone()).await;
            if is_notification {
                None
            } else {
                Some(serde_json::to_string(&JsonRpcSuccess::new(
                    request.id,
                    Value::String("subscribed".into()),
                ))
                .unwrap_or_default())
            }
        }
        methods::PING => {
            if is_notification {
                None
            } else {
                Some(serde_json::to_string(&JsonRpcSuccess::new(
                    request.id,
                    Value::String("pong".into()),
                ))
                .unwrap_or_default())
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
    let service = state.marketplace_service();
    match service.catalog().await {
        Ok(hits) => {
            let entries: Vec<Value> = hits
                .into_iter()
                .map(|hit| {
                    serde_json::json!({
                        "name": hit.entry.name,
                        "description": hit.entry.description,
                        "dcc": hit.entry.dcc,
                        "url": hit.entry.url,
                        "tags": hit.entry.tags,
                        "version": hit.entry.version,
                        "min_core_version": hit.entry.min_core_version,
                        "maintainer": hit.entry.maintainer,
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
    let service = state.marketplace_service();
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
        .map(|p| serde_json::from_value::<InstallParams>(p))
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default()
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid install params: {e}"),
            ))
            .unwrap_or_default()
        }
    };

    let operation_id = Uuid::new_v4().to_string();

    // Emit operation.progress: queued
    let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
        events::OPERATION_PROGRESS,
        serde_json::json!({
            "operation_id": operation_id,
            "phase": OperationPhase::Queued,
            "name": install_params.name,
            "dcc": install_params.dcc,
        }),
    ))
    .unwrap_or_default());

    let service = state.marketplace_service();
    let sources: Vec<String> = install_params.source.into_iter().collect();

    // Emit operation.progress: fetching
    let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
        events::OPERATION_PROGRESS,
        serde_json::json!({
            "operation_id": operation_id,
            "phase": OperationPhase::Fetching,
            "name": install_params.name,
            "dcc": install_params.dcc,
        }),
    ))
    .unwrap_or_default());

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
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::OPERATION_PROGRESS,
                serde_json::json!({
                    "operation_id": operation_id,
                    "phase": OperationPhase::Installing,
                    "name": install_params.name,
                    "dcc": install_params.dcc,
                }),
            ))
            .unwrap_or_default());

            if result.reload_required {
                // Emit operation.progress: reloading
                let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                    events::OPERATION_PROGRESS,
                    serde_json::json!({
                        "operation_id": operation_id,
                        "phase": OperationPhase::Reloading,
                        "name": install_params.name,
                        "dcc": install_params.dcc,
                    }),
                ))
                .unwrap_or_default());

                reload_skill_paths_and_refresh_backends(
                    &state.admin,
                    RefreshReason::ToolsListChanged,
                )
                .await;

                // Emit skills.reloaded
                let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                    events::SKILLS_RELOADED,
                    serde_json::json!({ "reason": "install" }),
                ))
                .unwrap_or_default());
            }

            // Emit operation.completed
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::OPERATION_COMPLETED,
                serde_json::json!({
                    "operation_id": operation_id,
                    "name": install_params.name,
                    "dcc": install_params.dcc,
                }),
            ))
            .unwrap_or_default());

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
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::OPERATION_FAILED,
                serde_json::json!({
                    "operation_id": operation_id,
                    "name": install_params.name,
                    "dcc": install_params.dcc,
                    "error": err.to_string(),
                }),
            ))
            .unwrap_or_default());

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
        .map(|p| serde_json::from_value::<UninstallParams>(p))
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default()
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid uninstall params: {e}"),
            ))
            .unwrap_or_default()
        }
    };

    let operation_id = Uuid::new_v4().to_string();

    let service = state.marketplace_service();
    match service.uninstall(&uninstall_params.name, &uninstall_params.dcc) {
        Ok(result) => {
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(
                    &state.admin,
                    RefreshReason::ToolsListChanged,
                )
                .await;

                let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                    events::SKILLS_RELOADED,
                    serde_json::json!({ "reason": "uninstall" }),
                ))
                .unwrap_or_default());
            }

            // Emit operation.completed
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::OPERATION_COMPLETED,
                serde_json::json!({
                    "operation_id": operation_id,
                    "name": uninstall_params.name,
                    "dcc": uninstall_params.dcc,
                }),
            ))
            .unwrap_or_default());

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
            let _ = state.events_tx.send(serde_json::to_string(&JsonRpcNotification::new(
                events::OPERATION_FAILED,
                serde_json::json!({
                    "operation_id": operation_id,
                    "name": uninstall_params.name,
                    "dcc": uninstall_params.dcc,
                    "error": err.to_string(),
                }),
            ))
            .unwrap_or_default());

            let (code, msg) = marketplace_error_to_rpc(&err);
            serde_json::to_string(&JsonRpcError::new(id, code, msg, None)).unwrap_or_default()
        }
    }
}

async fn handle_sources_list(state: &MarketplaceWsState, id: Option<Value>) -> String {
    let service = state.marketplace_service();
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
        .map(|p| serde_json::from_value::<AddSourceParams>(p))
        .transpose()
    {
        Ok(Some(p)) => p,
        Ok(None) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(id, "params required"))
                .unwrap_or_default()
        }
        Err(e) => {
            return serde_json::to_string(&JsonRpcError::invalid_params(
                id,
                &format!("invalid params: {e}"),
            ))
            .unwrap_or_default()
        }
    };

    let service = state.marketplace_service();
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
    state: &MarketplaceWsState,
    id: Option<Value>,
    _params: Option<Value>,
) -> String {
    // MarketplaceService doesn't have a remove_source method — reserved for future.
    let _ = state;
    serde_json::to_string(&JsonRpcError::new(
        id,
        METHOD_NOT_FOUND,
        "sources.remove not yet implemented".to_string(),
        None,
    ))
    .unwrap_or_default()
}

async fn handle_subscribe(
    state: &MarketplaceWsState,
    id: Option<Value>,
    params: Option<Value>,
) -> Option<()> {
    let subscribe_params: SubscribeParams = match params
        .map(|p| serde_json::from_value::<SubscribeParams>(p))
        .transpose()
    {
        Ok(Some(p)) => p,
        _ => {
            if let Some(id) = id {
                let err = JsonRpcError::invalid_params(
                    Some(id),
                    "topics array required",
                );
                let _ = state.events_tx.send(
                    serde_json::to_string(&err).unwrap_or_default(),
                );
            }
            return None;
        }
    };

    let topics: HashSet<String> = subscribe_params.topics.into_iter().collect();
    // Note: the current architecture broadcasts all events to all connections
    // via a single broadcast channel. Topic filtering is done at the client
    // side. The topics are stored for future fine-grained filtering.
    debug!(?topics, "WS client subscribed to topics");
    None // response handled by caller
}
