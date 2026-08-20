//! Shared gateway update-manifest loading.

use std::collections::HashMap;

use reqwest::header::ACCEPT;
use serde::Deserialize;
use thiserror::Error;

/// A single entry in the update manifest (binary_name -> entry).
#[derive(Debug, Clone, Deserialize)]
pub(crate) struct ManifestEntry {
    pub(crate) version: String,
    pub(crate) url: Option<String>,
    pub(crate) sha256: Option<String>,
    pub(crate) release_notes: Option<String>,
}

#[derive(Debug)]
pub(crate) struct VerifiedManifestAsset<'a> {
    pub(crate) url: &'a str,
    pub(crate) sha256: dcc_mcp_updater::Sha256Digest,
}

#[derive(Debug, Error)]
pub(crate) enum ManifestAssetError {
    #[error("Update manifest entry for {binary_name} is missing a download URL")]
    MissingUrl { binary_name: String },
    #[error(transparent)]
    Integrity(#[from] dcc_mcp_updater::UpdateError),
}

impl ManifestEntry {
    pub(crate) fn require_asset(
        &self,
        binary_name: &str,
    ) -> Result<VerifiedManifestAsset<'_>, ManifestAssetError> {
        let url = self
            .url
            .as_deref()
            .filter(|value| !value.trim().is_empty())
            .ok_or_else(|| ManifestAssetError::MissingUrl {
                binary_name: binary_name.to_owned(),
            })?;
        let sha256 = dcc_mcp_updater::Sha256Digest::require(binary_name, self.sha256.as_deref())?;
        Ok(VerifiedManifestAsset { url, sha256 })
    }
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
