//! Gateway update-version-check and download endpoints.
//!
//! These endpoints let CLI and server binaries query the gateway for
//! available updates through a version manifest hosted at a configurable URL.

use serde::Deserialize;

use super::*;

// ── Manifest types ───────────────────────────────────────────────────────────

/// A single entry in the update manifest (binary_name → entry).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ManifestEntry {
    pub(crate) version: String,
    pub(crate) url: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) release_notes: Option<String>,
}

/// Top-level update manifest fetched from `update_manifest_url`.
type UpdateManifest = std::collections::HashMap<String, ManifestEntry>;

// ── Error responses ──────────────────────────────────────────────────────────

fn not_configured() -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_IMPLEMENTED,
        Json(json!({
            "error": "update_manifest_url not configured",
            "hint": "set DCC_MCP_UPDATE_MANIFEST_URL or configure update_manifest_url on the gateway"
        })),
    )
}

fn not_found(binary: &str) -> (StatusCode, Json<Value>) {
    (
        StatusCode::NOT_FOUND,
        Json(json!({
            "error": format!("binary '{binary}' not found in update manifest"),
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
                Json(json!({"error": "missing required query parameter: binary"})),
            )
                .into_response();
        }
    };

    let current_version = query
        .current_version
        .clone()
        .unwrap_or_else(|| "0.0.0".to_string());

    // Fetch the manifest
    let manifest = match fetch_manifest(&state.http_client, &manifest_url).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "failed to fetch update manifest",
                    "detail": e.to_string(),
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

    let manifest = match fetch_manifest(&state.http_client, &manifest_url).await {
        Ok(m) => m,
        Err(e) => {
            return (
                StatusCode::BAD_GATEWAY,
                Json(json!({
                    "error": "failed to fetch update manifest",
                    "detail": e.to_string(),
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
                    "error": format!("no download URL configured for '{binary_name}'"),
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

// ── Helpers ──────────────────────────────────────────────────────────────────

/// Fetch and parse the update manifest from the configured URL.
async fn fetch_manifest(
    client: &reqwest::Client,
    url: &str,
) -> Result<UpdateManifest, reqwest::Error> {
    client
        .get(url)
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .json::<UpdateManifest>()
        .await
}
