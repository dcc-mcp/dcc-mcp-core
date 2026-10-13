//! Catalog observations, never installation or compatibility claims.

use dcc_mcp_catalog::{
    CatalogEntry, CatalogPackage, CatalogPolicy, CatalogRequirements, CatalogTarget,
};
use serde::Serialize;

pub const INSTALL_PROVIDER: &str = "official_install_catalog";
pub const MARKETPLACE_PROVIDER: &str = "official_marketplace";
pub const PROVIDER_IDS: [&str; 2] = [INSTALL_PROVIDER, MARKETPLACE_PROVIDER];

#[derive(Debug, Clone)]
pub struct DiscoveryRequest {
    pub offline: bool,
    pub query: String,
    pub target: Option<CatalogTarget>,
    pub provider: Option<String>,
    pub limit: usize,
}

/// Timestamps describe catalog evidence, not a component's publication date.
#[derive(Debug, Clone, Serialize)]
pub struct DiscoverySource {
    pub provider_id: String,
    pub url: String,
    pub state: &'static str,
    pub source: &'static str,
    pub freshness: &'static str,
    pub latest_checked: bool,
    pub checked_at_unix_secs: Option<u64>,
    pub cache_age_secs: Option<u64>,
    pub ttl_secs: Option<u64>,
    pub issued_at: Option<u64>,
    pub expires_at: Option<u64>,
    pub attested_at: Option<u64>,
    pub sha256: Option<String>,
    pub source_revision: Option<String>,
    pub attestation_identity: Option<String>,
    pub reason: Option<&'static str>,
    pub refresh_error: Option<&'static str>,
}

impl DiscoverySource {
    pub fn unavailable(provider_id: &str, url: &str, reason: &'static str) -> Self {
        Self {
            provider_id: provider_id.into(),
            url: url.into(),
            state: "unavailable",
            source: "unavailable",
            freshness: "unavailable",
            latest_checked: false,
            checked_at_unix_secs: None,
            cache_age_secs: None,
            ttl_secs: None,
            issued_at: None,
            expires_at: None,
            attested_at: None,
            sha256: None,
            source_revision: None,
            attestation_identity: None,
            reason: Some(reason),
            refresh_error: None,
        }
    }
}

#[derive(Debug, Clone)]
pub struct CatalogObservation {
    pub source: DiscoverySource,
    pub entries: Vec<CatalogEntry>,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentIdentity {
    pub provider_id: String,
    pub name: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct ComponentEvidence {
    pub provider_id: String,
    pub catalog_sha256: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
pub struct DiscoveredComponent {
    pub identity: ComponentIdentity,
    pub description: String,
    /// Only explicit package-format metadata can classify a component.
    pub component_type: String,
    pub targets: Vec<CatalogTarget>,
    pub advertised_version: Option<String>,
    pub core_requirement: Option<String>,
    pub min_core_version: Option<String>,
    pub requires: Option<CatalogRequirements>,
    pub package: Option<CatalogPackage>,
    pub policy: Option<CatalogPolicy>,
    pub url: Option<String>,
    pub availability: &'static str,
    pub published_at: Option<String>,
    pub installed: &'static str,
    pub runtime: &'static str,
    pub compatibility: &'static str,
    pub evidence: ComponentEvidence,
}

#[derive(Debug, Serialize)]
pub struct DiscoveryReport {
    pub schema_version: &'static str,
    pub read_only: bool,
    pub offline: bool,
    pub status: &'static str,
    pub query: String,
    pub target: Option<CatalogTarget>,
    pub total_matches: usize,
    pub count: usize,
    pub truncated: bool,
    pub providers: Vec<DiscoverySource>,
    pub components: Vec<DiscoveredComponent>,
}

pub fn assemble(
    request: DiscoveryRequest,
    observations: Vec<CatalogObservation>,
) -> DiscoveryReport {
    let available = observations
        .iter()
        .filter(|o| o.source.state == "available")
        .count();
    let incomplete = observations
        .iter()
        .any(|o| o.source.state != "available" || o.source.refresh_error.is_some());
    let status = if available == 0 {
        "unavailable"
    } else if incomplete {
        "partial"
    } else {
        "complete"
    };
    let mut ranked = Vec::new();
    for observation in &observations {
        if observation.source.state != "available" {
            continue;
        }
        for hit in dcc_mcp_catalog::search_hits(&observation.entries, &request.query) {
            let entry = &observation.entries[hit.index];
            let targets = dcc_mcp_marketplace::entry_targets(entry);
            if request.target.as_ref().is_some_and(|wanted| {
                !targets.iter().any(|target| {
                    target.kind == wanted.kind && target.id.eq_ignore_ascii_case(&wanted.id)
                })
            }) {
                continue;
            }
            let component_type = entry
                .package
                .as_ref()
                .and_then(|package| serde_json::to_value(package.format).ok())
                .and_then(|value| value.as_str().map(str::to_owned))
                .unwrap_or_else(|| "unknown".into());
            let provider_id = &observation.source.provider_id;
            ranked.push((
                hit.score,
                DiscoveredComponent {
                    identity: ComponentIdentity {
                        provider_id: provider_id.clone(),
                        name: entry.name.clone(),
                    },
                    description: entry.description.clone(),
                    component_type,
                    targets,
                    advertised_version: entry.version.clone(),
                    core_requirement: entry.core_requirement.clone(),
                    min_core_version: entry.min_core_version.clone(),
                    requires: entry.requires.clone(),
                    package: entry.package.clone(),
                    policy: entry.policy.clone(),
                    url: entry.url.clone(),
                    availability: "catalog_available",
                    published_at: None,
                    installed: "not_checked",
                    runtime: "not_checked",
                    compatibility: "not_checked",
                    evidence: ComponentEvidence {
                        provider_id: provider_id.clone(),
                        catalog_sha256: observation.source.sha256.clone(),
                    },
                },
            ));
        }
    }
    ranked.sort_by(|a, b| {
        b.0.cmp(&a.0)
            .then_with(|| a.1.identity.provider_id.cmp(&b.1.identity.provider_id))
            .then_with(|| a.1.identity.name.cmp(&b.1.identity.name))
    });
    let total_matches = ranked.len();
    let components: Vec<_> = ranked
        .into_iter()
        .take(request.limit)
        .map(|(_, entry)| entry)
        .collect();
    DiscoveryReport {
        schema_version: "dcc-mcp.ecosystem-discovery.v1",
        read_only: true,
        offline: request.offline,
        status,
        query: request.query,
        target: request.target,
        total_matches,
        count: components.len(),
        truncated: total_matches > components.len(),
        providers: observations.into_iter().map(|o| o.source).collect(),
        components,
    }
}

#[cfg(test)]
#[path = "ecosystem_discovery_tests.rs"]
mod tests;
