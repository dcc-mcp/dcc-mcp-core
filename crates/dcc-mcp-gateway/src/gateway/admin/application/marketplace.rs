//! Marketplace admin API handlers — PIP-521 / PIP-626 / PIP-699.
//!
//! Thin HTTP adapter that delegates to
//! [`dcc_mcp_marketplace::MarketplaceService`]. The response types below are
//! the HTTP contract with the admin-ui frontend and are intentionally kept
//! separate from the shared domain types.
//!
//! Exposes eight endpoints under `/admin/api/marketplace/`:
//! - `GET  /catalog`   — list available packages from marketplace sources
//! - `GET  /installed` — list installed packages
//! - `POST /install`   — install a package (supports optional `force: true`)
//! - `POST /uninstall` — uninstall a package
//! - `GET  /sources`   — list configured sources (builtin + config + env)
//! - `POST /sources`   — add a new source to the persistent config
//! - `GET  /outdated`  — list installed packages with outdated versions
//! - `POST /update`    — update one or all outdated packages

use axum::Json;
use axum::body::Body;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::{IntoResponse, Response};
use dcc_mcp_gateway_admin::{
    AddSourceRequest, InstallMetadataResponse, InstallRequestBody, InstallResultResponse,
    InstalledPackageResponse, MarketplaceEntryResponse, MarketplaceSourceResponse,
    OutdatedPackageResponse, OutdatedQueryParams, UninstallRequestBody, UninstallResultResponse,
    UpdateRequest, UpdateResultItem, resolve_marketplace_icon_url,
};
use dcc_mcp_marketplace::MarketplaceService;
use serde::Serialize;
use serde_json::json;

use super::super::state::AdminState;
use super::skill_reload::reload_skill_paths_and_refresh_backends;
use crate::gateway::capability::RefreshReason;

// ── Error envelope ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub(crate) struct ErrorResponse {
    pub(crate) kind: String,
    pub(crate) message: String,
}

impl ErrorResponse {
    pub(crate) fn from_error(err: &dcc_mcp_marketplace::MarketplaceError) -> Self {
        let kind = match err {
            dcc_mcp_marketplace::MarketplaceError::NotFound(_) => "not_found",
            dcc_mcp_marketplace::MarketplaceError::AlreadyInstalled { .. } => "already_installed",
            dcc_mcp_marketplace::MarketplaceError::DccMismatch { .. } => "dcc_mismatch",
            dcc_mcp_marketplace::MarketplaceError::AmbiguousDcc { .. } => "ambiguous_dcc",
            dcc_mcp_marketplace::MarketplaceError::MissingInstall(_) => "missing_install",
            dcc_mcp_marketplace::MarketplaceError::UnsupportedInstallType(_) => {
                "unsupported_install_type"
            }
            dcc_mcp_marketplace::MarketplaceError::MissingSkill(_) => "missing_skill",
            dcc_mcp_marketplace::MarketplaceError::CommandFailed(_) => "command_failed",
            dcc_mcp_marketplace::MarketplaceError::HashMismatch { .. } => "hash_mismatch",
            dcc_mcp_marketplace::MarketplaceError::Archive(..) => "archive_error",
            dcc_mcp_marketplace::MarketplaceError::InvalidPathComponent { .. } => {
                "invalid_path_component"
            }
            _ => "internal_error",
        };
        Self {
            kind: kind.to_string(),
            message: err.to_string(),
        }
    }
}

fn error_response(
    err: &dcc_mcp_marketplace::MarketplaceError,
    status: StatusCode,
) -> Response<Body> {
    let body = Json(json!({ "error": ErrorResponse::from_error(err) }));
    (status, body).into_response()
}

// ── Service helper ───────────────────────────────────────────────────────────

pub(crate) fn marketplace_service() -> MarketplaceService {
    let root = dcc_mcp_marketplace::marketplace_root_or_default();
    let config_path =
        dcc_mcp_marketplace::default_config_path().unwrap_or_else(|_| root.join("sources.json"));
    MarketplaceService::new(root).with_config_path(config_path)
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /admin/api/marketplace/catalog`
pub async fn handle_marketplace_catalog(State(_s): State<AdminState>) -> impl IntoResponse {
    let service = marketplace_service();
    match service.catalog().await {
        Ok(hits) => {
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
                    requires: hit.entry.requires,
                    icon: resolve_marketplace_icon_url(
                        hit.entry.icon.as_deref(),
                        Some(hit.source.url.as_str()),
                    ),
                    showcase: dcc_mcp_marketplace::resolve_catalog_asset_url(
                        hit.entry.showcase.as_deref(),
                        hit.entry.install.as_ref(),
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
            Json(json!({ "entries": entries })).into_response()
        }
        Err(err) => error_response(&err, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `GET /admin/api/marketplace/installed`
pub async fn handle_marketplace_installed(State(_s): State<AdminState>) -> impl IntoResponse {
    let service = marketplace_service();
    match service.list_installed(None) {
        Ok(list) => {
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
            Json(json!({ "packages": packages })).into_response()
        }
        Err(err) => error_response(&err, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `POST /admin/api/marketplace/install`
pub async fn handle_marketplace_install(
    State(s): State<AdminState>,
    Json(body): Json<InstallRequestBody>,
) -> impl IntoResponse {
    let service = marketplace_service();
    let sources: Vec<String> = body.source.into_iter().collect();
    match service
        .install(
            body.name.clone(),
            Some(body.dcc.clone()),
            sources,
            body.force,
            false,
        )
        .await
    {
        Ok(result) => {
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
            }
            Json(InstallResultResponse {
                installed: result.installed,
                name: result.name,
                dcc: result.dcc,
                version: result.version,
                path: result.path,
                skill_search_path: result.skill_search_path,
                install_type: result.install_type,
                reload_required: result.reload_required,
            })
            .into_response()
        }
        Err(err) => error_response(&err, StatusCode::BAD_REQUEST),
    }
}

/// `POST /admin/api/marketplace/uninstall`
pub async fn handle_marketplace_uninstall(
    State(s): State<AdminState>,
    Json(body): Json<UninstallRequestBody>,
) -> impl IntoResponse {
    let service = marketplace_service();
    match service.uninstall(&body.name, &body.dcc) {
        Ok(result) => {
            if result.reload_required {
                reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
            }
            Json(UninstallResultResponse {
                uninstalled: result.uninstalled,
                name: result.name,
                dcc: result.dcc,
                path: result.path,
                removed_state: result.removed_state,
                removed_files: result.removed_files,
                reload_required: result.reload_required,
            })
            .into_response()
        }
        Err(err) => error_response(&err, StatusCode::BAD_REQUEST),
    }
}

// ── New endpoints — PIP-699 M1 ────────────────────────────────────────────────

/// `GET /admin/api/marketplace/sources`
pub async fn handle_marketplace_sources(State(_s): State<AdminState>) -> impl IntoResponse {
    let service = marketplace_service();
    match service.list_sources() {
        Ok(sources) => {
            let items: Vec<MarketplaceSourceResponse> = sources
                .into_iter()
                .map(|s| MarketplaceSourceResponse {
                    name: s.name,
                    url: s.url,
                    origin: format!("{:?}", s.origin).to_lowercase(),
                })
                .collect();
            Json(json!({ "sources": items })).into_response()
        }
        Err(err) => error_response(&err, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `POST /admin/api/marketplace/sources`
pub async fn handle_marketplace_add_source(
    State(_s): State<AdminState>,
    Json(body): Json<AddSourceRequest>,
) -> impl IntoResponse {
    let service = marketplace_service();
    match service.add_source(&body.source) {
        Ok(sources) => {
            let items: Vec<MarketplaceSourceResponse> = sources
                .into_iter()
                .map(|s| MarketplaceSourceResponse {
                    name: s.name,
                    url: s.url,
                    origin: format!("{:?}", s.origin).to_lowercase(),
                })
                .collect();
            Json(json!({ "sources": items })).into_response()
        }
        Err(err) => error_response(&err, StatusCode::BAD_REQUEST),
    }
}

/// `GET /admin/api/marketplace/outdated`
pub async fn handle_marketplace_outdated(
    State(_s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<OutdatedQueryParams>,
) -> impl IntoResponse {
    let service = marketplace_service();
    match service
        .outdated(params.dcc.as_deref(), params.name.into_iter().collect())
        .await
    {
        Ok(list) => {
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
            Json(json!({ "dcc": list.dcc, "count": list.count, "packages": packages }))
                .into_response()
        }
        Err(err) => error_response(&err, StatusCode::INTERNAL_SERVER_ERROR),
    }
}

/// `POST /admin/api/marketplace/update`
pub async fn handle_marketplace_update(
    State(s): State<AdminState>,
    Json(body): Json<UpdateRequest>,
) -> impl IntoResponse {
    let service = marketplace_service();
    match service.update(body.name, body.all, body.dcc).await {
        Ok(results) => {
            let any_reload = results.iter().any(|r| r.reload_required);
            if any_reload {
                reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
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
            let count = items.len();
            Json(json!({ "updated": count, "results": items })).into_response()
        }
        Err(err) => error_response(&err, StatusCode::BAD_REQUEST),
    }
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use dcc_mcp_catalog::CatalogEntry;

    #[test]
    fn entry_targets_dcc_matches_case_insensitive() {
        let entry = CatalogEntry {
            name: "test".into(),
            description: "desc".into(),
            dcc: vec!["maya".into(), "blender".into()],
            targets: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            package: None,
            maintainer: None,
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        };
        assert!(dcc_mcp_marketplace::entry_targets_dcc(&entry, "Maya"));
        assert!(dcc_mcp_marketplace::entry_targets_dcc(&entry, "BLENDER"));
        assert!(!dcc_mcp_marketplace::entry_targets_dcc(&entry, "houdini"));
    }
}
