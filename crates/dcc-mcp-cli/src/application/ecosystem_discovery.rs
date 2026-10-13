//! Official catalog providers. No installation, host probes, or cache writes.

use std::future::Future;
use std::pin::Pin;
use std::time::{SystemTime, UNIX_EPOCH};

use crate::application::install::InstallService;
use crate::domain::ecosystem_discovery::{
    self, CatalogObservation, DiscoveryReport, DiscoveryRequest, DiscoverySource, INSTALL_PROVIDER,
    MARKETPLACE_PROVIDER,
};
use crate::domain::install_catalog::CatalogSource;

/// Extension boundary for independently verified catalog providers.
///
/// A provider must honor offline mode and supply catalog evidence only. The
/// executable registers official providers explicitly, never user source lists.
pub trait DiscoveryProvider {
    fn id(&self) -> &'static str;
    fn observe(
        &self,
        offline: bool,
        now: u64,
    ) -> Pin<Box<dyn Future<Output = CatalogObservation> + Send + '_>>;
}

pub struct OfficialInstallCatalog;
pub struct OfficialMarketplace;

impl DiscoveryProvider for OfficialInstallCatalog {
    fn id(&self) -> &'static str {
        INSTALL_PROVIDER
    }

    fn observe(
        &self,
        offline: bool,
        now: u64,
    ) -> Pin<Box<dyn Future<Output = CatalogObservation> + Send + '_>> {
        Box::pin(async move {
            let url = InstallService::DISCOVERY_SOURCE_URL;
            let (entries, provenance, refresh_error) =
                match InstallService::discovery_catalog(offline).await {
                    Ok(snapshot) => snapshot,
                    Err(reason) => return unavailable(INSTALL_PROVIDER, url, reason),
                };
            let mut source =
                DiscoverySource::unavailable(INSTALL_PROVIDER, url, "catalog_unavailable");
            source.state = "available";
            source.source = match provenance.source {
                CatalogSource::Remote => "remote",
                CatalogSource::Cache => "cache",
                CatalogSource::Bundled => "bundled",
                // Explicit local catalogs are outside the official discovery boundary.
                _ => return unavailable(INSTALL_PROVIDER, url, "unexpected_catalog_source"),
            };
            source.freshness = match provenance.source {
                CatalogSource::Remote => "fresh",
                CatalogSource::Cache => "valid_signed_cache",
                _ => "unknown",
            };
            source.reason = if provenance.source == CatalogSource::Bundled {
                Some("bundled_catalog_not_refreshed")
            } else {
                None
            };
            source.latest_checked = provenance.latest_checked;
            source.checked_at_unix_secs = provenance.latest_checked.then_some(now);
            source.cache_age_secs = provenance.latest_checked.then_some(0);
            // Signed catalog lifetime is authoritative; cache fetch age is not
            // stored by the existing install cache and must not be invented.
            source.ttl_secs = provenance
                .expires_at
                .zip(provenance.issued_at)
                .and_then(|(expires, issued)| expires.checked_sub(issued));
            source.issued_at = provenance.issued_at;
            source.expires_at = provenance.expires_at;
            source.sha256 = provenance.sha256;
            source.source_revision = provenance.source_revision;
            source.refresh_error = refresh_error;
            CatalogObservation { source, entries }
        })
    }
}

impl DiscoveryProvider for OfficialMarketplace {
    fn id(&self) -> &'static str {
        MARKETPLACE_PROVIDER
    }

    fn observe(
        &self,
        offline: bool,
        now: u64,
    ) -> Pin<Box<dyn Future<Output = CatalogObservation> + Send + '_>> {
        Box::pin(async move {
            let url = dcc_mcp_marketplace::OFFICIAL_MARKETPLACE_SOURCE;
            if offline {
                return unavailable(
                    MARKETPLACE_PROVIDER,
                    url,
                    "offline_no_verified_marketplace_snapshot",
                );
            }
            let verified = match dcc_mcp_marketplace::fetch_official_catalog().await {
                Ok(verified) => verified,
                Err(error) => return unavailable(MARKETPLACE_PROVIDER, url, error.code()),
            };
            let Some(attested_at) = verified
                .attestation
                .integrated_time
                .and_then(|time| u64::try_from(time).ok())
                .filter(|time| *time <= now.saturating_add(300))
            else {
                return unavailable(
                    MARKETPLACE_PROVIDER,
                    url,
                    "marketplace_attestation_time_invalid",
                );
            };
            let mut source =
                DiscoverySource::unavailable(MARKETPLACE_PROVIDER, url, "catalog_unavailable");
            source.state = "available";
            source.source = "remote";
            // The catalog has no signed expiry or publication timestamp. A
            // successful live read proves observation now, not recent publication.
            source.freshness = "live_observation";
            source.latest_checked = true;
            source.checked_at_unix_secs = Some(now);
            source.cache_age_secs = Some(0);
            source.attested_at = Some(attested_at);
            source.sha256 = Some(verified.attestation.sha256);
            source.attestation_identity = Some(verified.attestation.identity);
            source.reason = None;
            CatalogObservation {
                source,
                entries: verified.entries,
            }
        })
    }
}

fn unavailable(provider_id: &str, url: &str, reason: &'static str) -> CatalogObservation {
    CatalogObservation {
        source: DiscoverySource::unavailable(provider_id, url, reason),
        entries: Vec::new(),
    }
}

pub async fn discover(request: DiscoveryRequest) -> DiscoveryReport {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    discover_with_providers(
        &[&OfficialInstallCatalog, &OfficialMarketplace],
        request,
        now,
    )
    .await
}

pub async fn discover_with_providers(
    providers: &[&dyn DiscoveryProvider],
    request: DiscoveryRequest,
    now: u64,
) -> DiscoveryReport {
    let mut observations = Vec::new();
    for provider in providers {
        if request
            .provider
            .as_deref()
            .is_some_and(|id| id != provider.id())
        {
            continue;
        }
        observations.push(provider.observe(request.offline, now).await);
    }
    ecosystem_discovery::assemble(request, observations)
}

#[cfg(test)]
#[path = "ecosystem_discovery_tests.rs"]
mod tests;
