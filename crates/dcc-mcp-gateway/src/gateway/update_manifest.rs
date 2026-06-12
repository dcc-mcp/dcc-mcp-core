//! Shared gateway update-manifest loading.

use std::collections::HashMap;

use reqwest::header::ACCEPT;
use serde::Deserialize;

/// A single entry in the update manifest (binary_name -> entry).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ManifestEntry {
    pub(crate) version: String,
    pub(crate) url: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) release_notes: Option<String>,
}

/// Top-level update manifest fetched from `update_manifest_url`.
pub(crate) type UpdateManifest = HashMap<String, ManifestEntry>;

/// Fetch and parse the configured update manifest.
pub(crate) async fn fetch_update_manifest(
    client: &reqwest::Client,
    url: &str,
) -> Result<UpdateManifest, reqwest::Error> {
    client
        .get(url)
        .header(ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await?
        .error_for_status()?
        .json::<UpdateManifest>()
        .await
}
