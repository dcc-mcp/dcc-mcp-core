//! Shared gateway update-manifest loading.

use std::collections::HashMap;

use futures_util::StreamExt;
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

const MAX_MANIFEST_BYTES: usize = 1024 * 1024;
const MAX_ATTESTATION_BYTES: usize = 1024 * 1024;

#[derive(Debug, Error)]
pub(crate) enum UpdateManifestError {
    #[error("failed to fetch update metadata from '{url}': {source}")]
    Fetch {
        url: String,
        #[source]
        source: reqwest::Error,
    },
    #[error("update metadata from '{url}' exceeds the {limit}-byte safety limit")]
    TooLarge { url: String, limit: usize },
    #[error("update manifest from '{url}' is not valid UTF-8: {source}")]
    InvalidUtf8 {
        url: String,
        #[source]
        source: std::string::FromUtf8Error,
    },
    #[error("update manifest JSON is invalid: {0}")]
    Parse(#[from] serde_json::Error),
    #[error("official update manifest attestation verification failed: {0}")]
    Attestation(#[from] dcc_mcp_attestation::AttestationError),
}

/// Fetch and parse the configured update manifest.
pub(crate) async fn fetch_update_manifest(
    client: &reqwest::Client,
    url: &str,
) -> Result<UpdateManifest, UpdateManifestError> {
    let manifest_bytes = fetch_bounded(client, url, MAX_MANIFEST_BYTES).await?;
    if let Some(attestation_url) = official_attestation_url(url) {
        let bundle_bytes = fetch_bounded(client, &attestation_url, MAX_ATTESTATION_BYTES).await?;
        let bundle =
            String::from_utf8(bundle_bytes).map_err(|source| UpdateManifestError::InvalidUtf8 {
                url: attestation_url,
                source,
            })?;
        dcc_mcp_attestation::verify_attested_bytes(
            &manifest_bytes,
            &bundle,
            &dcc_mcp_attestation::GitHubAttestationPolicy::official_core_release(),
        )?;
    }
    let manifest =
        String::from_utf8(manifest_bytes).map_err(|source| UpdateManifestError::InvalidUtf8 {
            url: url.to_owned(),
            source,
        })?;
    Ok(serde_json::from_str(&manifest)?)
}

async fn fetch_bounded(
    client: &reqwest::Client,
    url: &str,
    limit: usize,
) -> Result<Vec<u8>, UpdateManifestError> {
    let response = client
        .get(url)
        .header(ACCEPT, "application/json")
        .timeout(std::time::Duration::from_secs(15))
        .send()
        .await
        .map_err(|source| UpdateManifestError::Fetch {
            url: url.to_owned(),
            source,
        })?
        .error_for_status()
        .map_err(|source| UpdateManifestError::Fetch {
            url: url.to_owned(),
            source,
        })?;
    if response
        .content_length()
        .is_some_and(|length| length > limit as u64)
    {
        return Err(UpdateManifestError::TooLarge {
            url: url.to_owned(),
            limit,
        });
    }
    let mut bytes = Vec::new();
    let mut stream = response.bytes_stream();
    while let Some(chunk) = stream.next().await {
        let chunk = chunk.map_err(|source| UpdateManifestError::Fetch {
            url: url.to_owned(),
            source,
        })?;
        if bytes.len().saturating_add(chunk.len()) > limit {
            return Err(UpdateManifestError::TooLarge {
                url: url.to_owned(),
                limit,
            });
        }
        bytes.extend_from_slice(&chunk);
    }
    Ok(bytes)
}

fn official_attestation_url(url: &str) -> Option<String> {
    let parsed = reqwest::Url::parse(url).ok()?;
    if parsed.scheme() != "https"
        || parsed.host_str() != Some("github.com")
        || parsed.query().is_some()
        || parsed.fragment().is_some()
    {
        return None;
    }
    let segments: Vec<_> = parsed.path_segments()?.collect();
    let file = match segments.as_slice() {
        [
            "dcc-mcp",
            "dcc-mcp-core",
            "releases",
            "latest",
            "download",
            file,
        ]
        | ["dcc-mcp", "dcc-mcp-core", "releases", "download", _, file] => *file,
        _ => return None,
    };
    let platform = file
        .strip_prefix("dcc-mcp-update-manifest-")?
        .strip_suffix(".json")?;
    if !matches!(
        platform,
        "linux-x86_64" | "windows-x86_64" | "macos-universal2"
    ) {
        return None;
    }
    let prefix = url.strip_suffix(file)?;
    Some(format!(
        "{prefix}dcc-mcp-update-manifest-{platform}.sigstore.json"
    ))
}

#[cfg(test)]
mod tests {
    use super::official_attestation_url;

    #[test]
    fn official_release_manifest_requires_its_detached_attestation() {
        assert_eq!(
            official_attestation_url(
                "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.json"
            )
            .as_deref(),
            Some(
                "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.sigstore.json"
            )
        );
        assert!(official_attestation_url(
            "https://github.com/dcc-mcp/dcc-mcp-core/releases/download/v0.21.0/dcc-mcp-update-manifest-linux-x86_64.json"
        )
        .is_some());
    }

    #[test]
    fn custom_and_lookalike_manifests_remain_operator_trusted() {
        for url in [
            "https://studio.example/update.json",
            "http://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.json",
            "https://github.com/attacker/dcc-mcp-core/releases/latest/download/dcc-mcp-update-manifest-windows-x86_64.json",
            "https://github.com/dcc-mcp/dcc-mcp-core/releases/latest/download/unexpected.json",
        ] {
            assert_eq!(official_attestation_url(url), None, "{url}");
        }
    }
}
