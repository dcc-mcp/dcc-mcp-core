//! Marketplace WebSocket API handler — PIP-617.
//!
//! Exposes a WebSocket endpoint under `/admin/api/marketplace/ws` to allow
//! standalone marketplace UI to interact with the local Gateway.

use axum::extract::State;
use axum::extract::ws::{Message, WebSocket, WebSocketUpgrade};
use axum::response::IntoResponse;
use futures::{SinkExt, StreamExt};
use serde::{Deserialize, Serialize};
use serde_json::json;
use tracing::{error, info};

use super::marketplace::{
    AddSourceRequest, ErrorResponse, InstallMetadataResponse, InstallRequestBody,
    InstallResultResponse, InstalledPackageResponse, MarketplaceEntryResponse,
    MarketplaceSourceResponse, OutdatedPackageResponse, OutdatedQueryParams, UninstallRequestBody,
    UninstallResultResponse, UpdateRequest, UpdateResultItem, marketplace_service,
    resolve_icon_url,
};
use super::skill_reload::reload_skill_paths_and_refresh_backends;
use super::state::AdminState;
use crate::gateway::capability::RefreshReason;

#[derive(Debug, Deserialize)]
struct WsRequest {
    id: String,
    action: String,
    #[serde(default)]
    payload: serde_json::Value,
}

#[derive(Debug, Serialize)]
struct WsResponse {
    id: String,
    status: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    payload: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    error: Option<WsError>,
}

#[derive(Debug, Serialize)]
struct WsError {
    kind: String,
    message: String,
}

pub async fn handle_marketplace_ws(
    ws: WebSocketUpgrade,
    State(state): State<AdminState>,
) -> impl IntoResponse {
    ws.on_upgrade(move |socket| handle_socket(socket, state))
}

async fn handle_socket(socket: WebSocket, state: AdminState) {
    info!("Marketplace WebSocket client connected");
    let (mut sender, mut receiver) = socket.split();

    while let Some(Ok(msg)) = receiver.next().await {
        let text = match msg {
            Message::Text(t) => t,
            Message::Close(_) => break,
            _ => continue,
        };

        let req: WsRequest = match serde_json::from_str(&text) {
            Ok(r) => r,
            Err(err) => {
                error!("Failed to parse WebSocket request: {}", err);
                continue;
            }
        };

        let res = match handle_action(&req.action, req.payload, &state).await {
            Ok(val) => WsResponse {
                id: req.id,
                status: "success".to_string(),
                payload: Some(val),
                error: None,
            },
            Err((kind, msg)) => WsResponse {
                id: req.id,
                status: "error".to_string(),
                payload: None,
                error: Some(WsError { kind, message: msg }),
            },
        };

        let res_text = match serde_json::to_string(&res) {
            Ok(t) => t,
            Err(err) => {
                error!("Failed to serialize WebSocket response: {}", err);
                continue;
            }
        };

        if let Err(err) = sender.send(Message::Text(res_text.into())).await {
            error!("Failed to send WebSocket message: {}", err);
            break;
        }
    }
    info!("Marketplace WebSocket client disconnected");
}

async fn handle_action(
    action: &str,
    payload: serde_json::Value,
    state: &AdminState,
) -> Result<serde_json::Value, (String, String)> {
    let service = marketplace_service();
    match action {
        "catalog" => {
            let hits = service.catalog().await.map_err(|err| {
                let err_res = ErrorResponse::from_error(&err);
                (err_res.kind, err_res.message)
            })?;
            let entries: Vec<MarketplaceEntryResponse> = hits
                .into_iter()
                .map(|hit| MarketplaceEntryResponse {
                    name: hit.entry.name,
                    description: hit.entry.description,
                    dcc: hit.entry.dcc,
                    url: hit.entry.url,
                    tags: hit.entry.tags,
                    version: hit.entry.version,
                    min_core_version: hit.entry.min_core_version,
                    maintainer: hit.entry.maintainer,
                    icon: resolve_icon_url(
                        hit.entry.icon.as_deref(),
                        Some(hit.source.url.as_str()),
                    ),
                    source_name: Some(hit.source.name),
                    source_url: Some(hit.source.url),
                    install: hit.entry.install.as_ref().map(|i| InstallMetadataResponse {
                        install_type: i.install_type.clone(),
                        url: i.url.clone(),
                        ref_: i.ref_.clone(),
                    }),
                })
                .collect();
            Ok(json!({ "entries": entries }))
        }
        "installed" => {
            let list = service.list_installed(None).map_err(|err| {
                let err_res = ErrorResponse::from_error(&err);
                (err_res.kind, err_res.message)
            })?;
            let packages: Vec<InstalledPackageResponse> = list
                .packages
                .into_iter()
                .map(|p| InstalledPackageResponse {
                    name: p.name,
                    dcc: p.dcc,
                    version: p.version,
                    path: p.path,
                    source_name: p.source_name,
                    source_url: p.source_url,
                    install_type: p.install_type,
                    install_url: p.install_url,
                    install_ref: p.install_ref,
                    installed_at_ms: p.installed_at_ms,
                })
                .collect();
            Ok(json!({ "packages": packages }))
        }
        "install" => {
            let body: InstallRequestBody = serde_json::from_value(payload)
                .map_err(|err| ("bad_request".to_string(), err.to_string()))?;
            let sources: Vec<String> = body.source.into_iter().collect();
            let result = service
                .install(
                    body.name.clone(),
                    Some(body.dcc.clone()),
                    sources,
                    body.force,
                    false,
                )
                .await
                .map_err(|err| {
                    let err_res = ErrorResponse::from_error(&err);
                    (err_res.kind, err_res.message)
                })?;
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(state, RefreshReason::ToolsListChanged)
                    .await;
            }
            Ok(json!(InstallResultResponse {
                installed: result.installed,
                name: result.name,
                dcc: result.dcc,
                version: result.version,
                path: result.path,
                skill_search_path: result.skill_search_path,
                install_type: result.install_type,
                reload_required: result.reload_required,
            }))
        }
        "uninstall" => {
            let body: UninstallRequestBody = serde_json::from_value(payload)
                .map_err(|err| ("bad_request".to_string(), err.to_string()))?;
            let result = service.uninstall(&body.name, &body.dcc).map_err(|err| {
                let err_res = ErrorResponse::from_error(&err);
                (err_res.kind, err_res.message)
            })?;
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(state, RefreshReason::ToolsListChanged)
                    .await;
            }
            Ok(json!(UninstallResultResponse {
                uninstalled: result.uninstalled,
                name: result.name,
                dcc: result.dcc,
                path: result.path,
                removed_state: result.removed_state,
                removed_files: result.removed_files,
                reload_required: result.reload_required,
            }))
        }
        "sources" => {
            let sources = service.list_sources().map_err(|err| {
                let err_res = ErrorResponse::from_error(&err);
                (err_res.kind, err_res.message)
            })?;
            let items: Vec<MarketplaceSourceResponse> = sources
                .into_iter()
                .map(|s| MarketplaceSourceResponse {
                    name: s.name,
                    url: s.url,
                    origin: format!("{:?}", s.origin).to_lowercase(),
                })
                .collect();
            Ok(json!({ "sources": items }))
        }
        "add_source" => {
            let body: AddSourceRequest = serde_json::from_value(payload)
                .map_err(|err| ("bad_request".to_string(), err.to_string()))?;
            let sources = service.add_source(&body.source).map_err(|err| {
                let err_res = ErrorResponse::from_error(&err);
                (err_res.kind, err_res.message)
            })?;
            let items: Vec<MarketplaceSourceResponse> = sources
                .into_iter()
                .map(|s| MarketplaceSourceResponse {
                    name: s.name,
                    url: s.url,
                    origin: format!("{:?}", s.origin).to_lowercase(),
                })
                .collect();
            Ok(json!({ "sources": items }))
        }
        "outdated" => {
            let params: OutdatedQueryParams = serde_json::from_value(payload)
                .map_err(|err| ("bad_request".to_string(), err.to_string()))?;
            let list = service
                .outdated(params.dcc.as_deref(), params.name.into_iter().collect())
                .await
                .map_err(|err| {
                    let err_res = ErrorResponse::from_error(&err);
                    (err_res.kind, err_res.message)
                })?;
            let packages: Vec<OutdatedPackageResponse> = list
                .packages
                .into_iter()
                .map(|p| OutdatedPackageResponse {
                    name: p.name,
                    dcc: p.dcc,
                    installed_version: p.installed_version,
                    latest_version: p.latest_version,
                    source_name: p.source_name,
                    source_url: p.source_url,
                    install_type: p.install_type,
                    install_url: p.install_url,
                    install_ref: p.install_ref,
                    path: p.path,
                })
                .collect();
            Ok(json!({ "dcc": list.dcc, "count": list.count, "packages": packages }))
        }
        "update" => {
            let body: UpdateRequest = serde_json::from_value(payload)
                .map_err(|err| ("bad_request".to_string(), err.to_string()))?;
            let results = service
                .update(body.name, body.all, body.dcc)
                .await
                .map_err(|err| {
                    let err_res = ErrorResponse::from_error(&err);
                    (err_res.kind, err_res.message)
                })?;
            let any_reload = results.iter().any(|r| r.reload_required);
            if any_reload {
                reload_skill_paths_and_refresh_backends(state, RefreshReason::ToolsListChanged)
                    .await;
            }
            let items: Vec<UpdateResultItem> = results
                .into_iter()
                .map(|r| UpdateResultItem {
                    updated: r.updated,
                    name: r.name,
                    dcc: r.dcc,
                    previous_version: r.previous_version,
                    new_version: r.new_version,
                    path: r.path,
                    install_type: r.install_type,
                    source_name: r.source_name,
                    source_url: r.source_url,
                    reload_required: r.reload_required,
                })
                .collect();
            Ok(json!({ "updated": items.len(), "results": items }))
        }
        _ => Err((
            "unknown_action".to_string(),
            format!("Unknown action: {}", action),
        )),
    }
}
