//! Gateway update-version-check and download endpoints.
//!
//! These endpoints let CLI and server binaries query the gateway for
//! available updates through a version manifest hosted at a configurable URL.

use serde::Deserialize;

use super::*;
use crate::gateway::update_manifest::fetch_update_manifest;

// ── Error responses ──────────────────────────────────────────────────────────

fn not_configured() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "status": "not_configured",
            "error": "update_manifest_url_not_configured",
            "message": "Update manifest URL is not configured for this gateway.",
            "hint": "Set DCC_MCP_UPDATE_MANIFEST_URL or configure update_manifest_url on the gateway.",
            "update_available": false,
        })),
    )
}

fn not_found(binary: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "status": "binary_not_found",
            "error": "binary_not_found",
            "message": format!("Binary '{binary}' was not found in the update manifest."),
            "binary_name": binary,
            "update_available": false,
        })),
    )
}

// ── Query params ─────────────────────────────────────────────────────────────

#[derive(Debug, Deserialize)]
pub(crate) struct CheckQuery {
    pub(crate) binary: Option<String>,
    pub(crate) current_version: Option<String>,
}

// ── Handlers ─────────────────────────────────────────────────────────────────

/// `GET /v1/update/check?binary={name}&current_version={ver}`
///
/// Checks whether a newer version is available for the given binary.
/// Returns the latest version info if the manifest is accessible.
pub(crate) async fn handle_v1_update_check(
    State(state): State<GatewayState>,
    Query(query): Query<CheckQuery>,
) -> Response {
    let manifest_url = match &state.update_manifest_url {
        Some(url) => url.clone(),
        None => return not_configured().into_response(),
    };

    let binary_name = match &query.binary {
        Some(name) => name.clone(),
        None => {
            return (
                StatusCode::BAD_REQUEST,
                Json(json!({
                    "status": "bad_request",
                    "error": "missing_binary",
                    "message": "Missing required query parameter: binary.",
                    "update_available": false,
                })),
            )
                .into_response();
        }
    };

    let current_version = query
        .current_version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string());

    // Fetch the manifest
    let manifest = match fetch_update_manifest(&state.http_client, &manifest_url).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "status": "manifest_error",
                    "error": "failed_to_fetch_update_manifest",
                    "message": e.to_string(),
                    "update_available": false,
                })),
            )
                .into_response();
        }
    };

    let entry = match manifest.get(&binary_name) {
        Some(e) => e,
        None => return not_found(&binary_name).into_response(),
    };

    let update_available = is_newer_version(&entry.version, &current_version);

    let resp = json!({
        "update_available": update_available,
        "latest_version": entry.version,
        "download_url": entry.url,
        "sha256": entry.sha256,
        "release_notes": entry.release_notes,
        "current_version": current_version,
    });

    (StatusCode::OK, Json(resp)).into_response()
}

/// `GET /v1/update/download/{binary_name}`
///
/// Returns the download URL for the latest version of the given binary.
pub(crate) async fn handle_v1_update_download(
    State(state): State<GatewayState>,
    Path(binary_name): Path<String>,
) -> Response {
    let manifest_url = match &state.update_manifest_url {
        Some(url) => url.clone(),
        None => return not_configured().into_response(),
    };

    let manifest = match fetch_update_manifest(&state.http_client, &manifest_url).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "status": "manifest_error",
                    "error": "failed_to_fetch_update_manifest",
                    "message": e.to_string(),
                    "update_available": false,
                })),
            )
                .into_response();
        }
    };

    let entry = match manifest.get(&binary_name) {
        Some(e) => e,
        None => return not_found(&binary_name).into_response(),
    };

    let download_url = match &entry.url {
        Some(url) => url.clone(),
        None => {
            return (
                StatusCode::NOT_FOUND,
                Json(json!({
                    "status": "download_url_not_configured",
                    "error": "download_url_not_configured",
                    "message": format!("No download URL is configured for binary '{binary_name}'."),
                    "binary_name": binary_name,
                    "latest_version": entry.version,
                    "update_available": false,
                })),
            )
                .into_response();
        }
    };

    (
        StatusCode::OK,
        Json(json!({ "download_url": download_url })),
    )
        .into_response()
}
