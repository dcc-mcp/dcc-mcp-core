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
use dcc_mcp_marketplace::MarketplaceService;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::skill_reload::reload_skill_paths_and_refresh_backends;
use super::state::AdminState;
use crate::gateway::capability::RefreshReason;

// ── Response types (HTTP contract with admin-ui frontend) ────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntryResponse {
    pub name: String,
    pub description: String,
    pub dcc: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_core_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallMetadataResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallMetadataResponse {
    #[serde(rename = "type")]
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackageResponse {
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_ref: Option<String>,
    pub installed_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResultResponse {
    pub installed: bool,
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub skill_search_path: String,
    pub install_type: String,
    pub reload_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallResultResponse {
    pub uninstalled: bool,
    pub name: String,
    pub dcc: String,
    pub path: String,
    pub removed_state: bool,
    pub removed_files: bool,
    pub reload_required: bool,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequestBody {
    pub name: String,
    pub dcc: String,
    #[serde(default)]
    pub source: Option<String>,
    /// Force re-install even if the package already exists at the destination.
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct UninstallRequestBody {
    pub name: String,
    pub dcc: String,
}

#[derive(Debug, Serialize)]
pub struct SourcesResponse {
    pub sources: Vec<MarketplaceSourceResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketplaceSourceResponse {
    pub name: String,
    pub url: String,
    pub origin: String,
}

#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    pub source: String,
}

#[derive(Debug, Serialize)]
pub struct OutdatedResponse {
    pub dcc: Option<String>,
    pub count: usize,
    pub packages: Vec<OutdatedPackageResponse>,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutdatedPackageResponse {
    pub name: String,
    pub dcc: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    pub install_url: Option<String>,
    pub install_ref: Option<String>,
    pub path: String,
}

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    /// Optional package name for single-package update.
    pub name: Option<String>,
    /// Required when updating a single package by name.
    pub dcc: Option<String>,
    /// Update all outdated packages.
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateResponse {
    pub updated: usize,
    pub results: Vec<UpdateResultItem>,
}

#[derive(Debug, Serialize)]
pub struct UpdateResultItem {
    pub updated: bool,
    pub name: String,
    pub dcc: String,
    pub previous_version: Option<String>,
    pub new_version: Option<String>,
    pub path: String,
    pub install_type: String,
    pub source_name: String,
    pub source_url: String,
    pub reload_required: bool,
}

// ── Error envelope ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
struct ErrorResponse {
    kind: String,
    message: String,
}

impl ErrorResponse {
    fn from_error(err: &dcc_mcp_marketplace::MarketplaceError) -> Self {
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

fn marketplace_service() -> MarketplaceService {
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

#[derive(Debug, Deserialize)]
pub struct OutdatedQueryParams {
    #[serde(default)]
    name: Option<String>,
    #[serde(default)]
    dcc: Option<String>,
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

/// Resolve an icon path to a full URL.
///
/// Absolute URLs are passed through unchanged. Relative paths are resolved
/// against raw.githubusercontent.com source URLs.
fn resolve_icon_url(icon: Option<&str>, source_url: Option<&str>) -> Option<String> {
    let icon = icon?;
    if icon.starts_with("http://") || icon.starts_with("https://") {
        return Some(icon.to_string());
    }
    let source_url = source_url?;
    if source_url.contains("raw.githubusercontent.com")
        && let Some(base) = source_url.rsplit_once('/')
    {
        return Some(format!("{}/{}", base.0, icon.trim_start_matches('/')));
    }
    Some(icon.to_string())
}

// ── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use dcc_mcp_catalog::CatalogEntry;

    use super::*;

    #[test]
    fn entry_targets_dcc_matches_case_insensitive() {
        let entry = CatalogEntry {
            name: "test".into(),
            description: "desc".into(),
            dcc: vec!["maya".into(), "blender".into()],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
            icon: None,
        };
        assert!(dcc_mcp_marketplace::entry_targets_dcc(&entry, "Maya"));
        assert!(dcc_mcp_marketplace::entry_targets_dcc(&entry, "BLENDER"));
        assert!(!dcc_mcp_marketplace::entry_targets_dcc(&entry, "houdini"));
    }

    #[test]
    fn resolve_icon_url_none_when_icon_is_none() {
        assert_eq!(resolve_icon_url(None, None), None);
    }

    #[test]
    fn resolve_icon_url_passes_through_absolute_url() {
        let icon = "https://cdn.example.com/icons/my-icon.png";
        assert_eq!(resolve_icon_url(Some(icon), None), Some(icon.to_string()));
    }

    #[test]
    fn resolve_icon_url_resolves_relative_against_raw_github() {
        let source_url =
            "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya-mgear/main/marketplace.json";
        assert_eq!(
            resolve_icon_url(Some("icon.png"), Some(source_url)),
            Some(
                "https://raw.githubusercontent.com/dcc-mcp/dcc-mcp-maya-mgear/main/icon.png".into()
            )
        );
    }

    #[test]
    fn resolve_icon_url_passes_through_relative_without_raw_github_source() {
        assert_eq!(
            resolve_icon_url(Some("icon.png"), Some("https://example.com/catalog.json")),
            Some("icon.png".into())
        );
    }
}
