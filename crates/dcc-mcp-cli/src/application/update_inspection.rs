//! Bounded official release reads; no updater, installer, or gateway lifecycle.

use std::future::Future;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use serde::Deserialize;

use crate::application::package_manager;
use crate::application::update::{CLI_VERSION, ReadOnlyCliUpdateCache, read_cli_update_cache};
use crate::domain::update_inspection::{
    self, CORE_REPOSITORY, Constraints, RELEASE_TTL_SECS, ReleaseObservation, SourceObservation,
    UpdateInspection, VersionObservation, Versions,
};

const RELEASE_API: &str = "https://api.github.com/repos/dcc-mcp/dcc-mcp-core/releases/latest";
const MAX_RELEASE_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone)]
pub struct InspectRequest {
    pub offline: bool,
    pub channel: String,
    pub issue_url: Option<String>,
}

/// Providers supply observations only; applying a release is outside this API.
pub trait ReleaseProvider {
    fn stable_release(&self) -> impl Future<Output = Result<ReleaseObservation, &'static str>>;
}

pub struct OfficialReleaseProvider;

#[derive(Deserialize)]
struct GitHubRelease {
    tag_name: String,
    html_url: String,
    draft: bool,
    prerelease: bool,
    published_at: Option<String>,
    body: Option<String>,
}

impl ReleaseProvider for OfficialReleaseProvider {
    async fn stable_release(&self) -> Result<ReleaseObservation, &'static str> {
        let client = reqwest::Client::builder()
            .connect_timeout(Duration::from_secs(5))
            .timeout(Duration::from_secs(10))
            .redirect(reqwest::redirect::Policy::none())
            .build()
            .map_err(|_| "release_client_unavailable")?;
        let mut response = client
            .get(RELEASE_API)
            .header(reqwest::header::USER_AGENT, "dcc-mcp-cli-update-inspection")
            .header(reqwest::header::ACCEPT, "application/vnd.github+json")
            .send()
            .await
            .map_err(|_| "official_release_unreachable")?;
        if !response.status().is_success() {
            return Err(
                if response.status().as_u16() == 403 || response.status().as_u16() == 429 {
                    "official_release_rate_limited"
                } else {
                    "official_release_http_error"
                },
            );
        }
        if response
            .content_length()
            .is_some_and(|size| size > MAX_RELEASE_BYTES as u64)
        {
            return Err("official_release_too_large");
        }
        let mut bytes = Vec::new();
        while let Some(chunk) = response
            .chunk()
            .await
            .map_err(|_| "official_release_read_failed")?
        {
            if bytes.len().saturating_add(chunk.len()) > MAX_RELEASE_BYTES {
                return Err("official_release_too_large");
            }
            bytes.extend_from_slice(&chunk);
        }
        decode_release(&bytes)
    }
}

fn decode_release(bytes: &[u8]) -> Result<ReleaseObservation, &'static str> {
    let release: GitHubRelease =
        serde_json::from_slice(bytes).map_err(|_| "official_release_invalid_json")?;
    let version = release
        .tag_name
        .strip_prefix('v')
        .unwrap_or(&release.tag_name);
    let parsed = semver::Version::parse(version).map_err(|_| "official_release_invalid_version")?;
    if release.draft || release.prerelease || !parsed.pre.is_empty() {
        return Err("official_release_is_not_stable");
    }
    if release.html_url != format!("{CORE_REPOSITORY}/releases/tag/{}", release.tag_name)
        || release
            .published_at
            .as_ref()
            .is_none_or(|value| value.is_empty())
    {
        return Err("official_release_identity_invalid");
    }
    Ok(ReleaseObservation {
        version: version.to_owned(),
        url: Some(release.html_url),
        published_at: release.published_at,
        notes: release.body,
    })
}

pub async fn inspect_current_cli(request: InspectRequest) -> UpdateInspection {
    let installed = package_manager::detect()
        .filter(|marker| marker.distribution == "dcc-mcp-cli")
        .and_then(|marker| {
            semver::Version::parse(&marker.version)
                .ok()
                .map(|_| marker.version)
        })
        .map(|version| VersionObservation::known(version, "package_manager_record"))
        .unwrap_or_else(|| {
            VersionObservation::unknown("package_manager_record_missing_or_invalid")
        });
    let versions = Versions {
        installed,
        runtime: VersionObservation::known(CLI_VERSION, "executing_cli_build"),
        bundled_core: VersionObservation::known(CLI_VERSION, "cli_workspace_build"),
    };
    inspect_with_provider(
        &OfficialReleaseProvider,
        request,
        versions,
        read_cli_update_cache(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    )
    .await
}

pub async fn inspect_with_provider(
    provider: &impl ReleaseProvider,
    request: InspectRequest,
    versions: Versions,
    cache: Option<ReadOnlyCliUpdateCache>,
    now: u64,
) -> UpdateInspection {
    let unavailable_reason = if request.channel != "stable" {
        "unsupported_channel"
    } else if request.offline {
        "offline_no_official_release_snapshot"
    } else {
        match provider.stable_release().await {
            Ok(release) => {
                let source = SourceObservation {
                    provider: "official_github_release",
                    url: Some(RELEASE_API.into()),
                    checked_at_unix_secs: Some(now),
                    cache_age_secs: Some(0),
                    ttl_secs: RELEASE_TTL_SECS,
                    offline: false,
                    reason: None,
                    refresh_error: None,
                };
                return update_inspection::inspect(
                    versions,
                    &request.channel,
                    source,
                    Some(release),
                    Constraints::default(),
                    request.issue_url.as_deref(),
                );
            }
            Err(reason) => reason,
        }
    };
    let mut source = SourceObservation {
        provider: "unavailable",
        url: Some(RELEASE_API.into()),
        checked_at_unix_secs: None,
        cache_age_secs: None,
        ttl_secs: RELEASE_TTL_SECS,
        offline: request.offline,
        reason: Some(unavailable_reason),
        refresh_error: (!request.offline && request.channel == "stable")
            .then_some(unavailable_reason),
    };
    let release = cache.filter(|_| request.channel == "stable").map(|cache| {
        source.provider = "gateway_update_cache";
        source.url = None;
        source.checked_at_unix_secs = Some(cache.checked_at_unix_secs);
        source.cache_age_secs = now.checked_sub(cache.checked_at_unix_secs);
        source.reason = Some("official_release_provenance_not_recorded");
        ReleaseObservation {
            version: cache.latest_version,
            url: None,
            published_at: None,
            notes: None,
        }
    });
    update_inspection::inspect(
        versions,
        &request.channel,
        source,
        release,
        Constraints::default(),
        request.issue_url.as_deref(),
    )
}

#[cfg(test)]
#[path = "update_inspection_tests.rs"]
mod tests;
