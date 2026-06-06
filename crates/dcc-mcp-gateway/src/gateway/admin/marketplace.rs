//! Marketplace admin API handlers — PIP-521 / PIP-626.
//!
//! Thin HTTP adapter that delegates to
//! [`dcc_mcp_marketplace::MarketplaceService`]. The response types below are
//! the HTTP contract with the admin-ui frontend and are intentionally kept
//! separate from the shared domain types.
//!
//! Exposes four endpoints under `/admin/api/marketplace/`:
//! - `GET  /catalog`   — list available packages from marketplace sources
//! - `GET  /installed` — list installed packages
//! - `POST /install`   — install a package
//! - `POST /uninstall` — uninstall a package

use std::path::PathBuf;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
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
}

#[derive(Debug, Deserialize)]
pub struct UninstallRequestBody {
    pub name: String,
    pub dcc: String,
}

// ── Service helper ───────────────────────────────────────────────────────────

fn marketplace_service() -> MarketplaceService {
    let root = marketplace_root();
    MarketplaceService::new(root)
}

fn marketplace_root() -> PathBuf {
    if let Ok(root) = std::env::var("DCC_MCP_MARKETPLACE_INSTALL_ROOT")
        && !root.trim().is_empty()
    {
        return PathBuf::from(root);
    }
    home_dir().join(".dcc-mcp").join("marketplace")
}

fn home_dir() -> PathBuf {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."))
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
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
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
        Err(err) => (
            StatusCode::INTERNAL_SERVER_ERROR,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
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
        .install(body.name.clone(), Some(body.dcc.clone()), sources, false)
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
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
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
        Err(err) => (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": err.to_string() })),
        )
            .into_response(),
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
