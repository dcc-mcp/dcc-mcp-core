//! Shared marketplace domain types.
//!
//! These are the canonical types used by both the CLI and the Gateway admin
//! panel. The Gateway maps them to HTTP response types in its own adapter layer.

use dcc_mcp_catalog::CatalogEntry;
use serde::{Deserialize, Serialize};

/// Directory used for marketplace packages that serve every DCC host.
pub const HOST_NEUTRAL_DCC: &str = "any";

/// CLI spellings that request the shared host-neutral install directory.
pub const SHARED_DCC_REQUESTS: [&str; 2] = [HOST_NEUTRAL_DCC, "all"];

// ── source ────────────────────────────────────────────────────────────────────

/// Canonical URL for the official dcc-mcp/marketplace catalog.
pub const OFFICIAL_MARKETPLACE_SOURCE: &str =
    "https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json";

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceSource {
    pub name: String,
    pub url: String,
    pub origin: MarketplaceSourceOrigin,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketplaceSourceOrigin {
    Builtin,
    Config,
    Env,
    Explicit,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct StoredMarketplaceSource {
    pub name: String,
    pub url: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MarketplaceSourceConfig {
    #[serde(default)]
    pub sources: Vec<StoredMarketplaceSource>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceHit {
    pub source: MarketplaceSource,
    pub entry: CatalogEntry,
}

// ── search / inspect ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceSearchResult {
    pub query: Option<String>,
    pub dcc: Option<String>,
    pub count: usize,
    pub hits: Vec<MarketplaceHit>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceInspectResult {
    pub name: String,
    pub count: usize,
    pub matches: Vec<MarketplaceHit>,
}

// ── install / uninstall results ──────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceInstallResult {
    pub installed: bool,
    pub name: String,
    pub dcc: String,
    pub version: Option<String>,
    pub path: String,
    pub skill_search_path: String,
    pub source: MarketplaceSource,
    pub entry: CatalogEntry,
    pub install_type: String,
    /// Existing host-specific copies that can shadow this shared install.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub superseded: Vec<String>,
    /// Immutable git revision that was actually installed, when the source supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    pub reload_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceUninstallResult {
    pub uninstalled: bool,
    pub name: String,
    pub dcc: String,
    pub path: String,
    pub removed_state: bool,
    pub removed_files: bool,
    pub reload_required: bool,
}

// ── installed state ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstalledMarketplacePackage {
    pub name: String,
    pub dcc: String,
    pub version: Option<String>,
    pub path: String,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    pub install_url: Option<String>,
    pub install_ref: Option<String>,
    /// Immutable git revision resolved during installation.
    ///
    /// Older state files omit this field and remain readable.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    pub installed_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MarketplaceInstalledState {
    #[serde(default)]
    pub packages: Vec<InstalledMarketplacePackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceInstalledList {
    pub dcc: Option<String>,
    pub count: usize,
    pub packages: Vec<InstalledMarketplacePackage>,
}

// ── outdated / update ────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct OutdatedMarketplacePackage {
    pub name: String,
    pub dcc: String,
    pub installed_version: Option<String>,
    pub latest_version: Option<String>,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    pub install_url: Option<String>,
    pub install_ref: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub installed_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_commit: Option<String>,
    pub path: String,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceOutdatedList {
    pub dcc: Option<String>,
    pub count: usize,
    pub packages: Vec<OutdatedMarketplacePackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct MarketplaceUpdateResult {
    pub updated: bool,
    pub name: String,
    pub dcc: String,
    pub previous_version: Option<String>,
    pub new_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub previous_commit: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub new_commit: Option<String>,
    pub path: String,
    pub install_type: String,
    pub source_name: String,
    pub source_url: String,
    pub reload_required: bool,
}

// ── add-repo (direct GitHub install) ──────────────────────────────────────────

/// A single skill discovered in a GitHub repo via SKILL.md discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoSkillInfo {
    pub name: String,
    pub description: Option<String>,
    pub dcc: Option<String>,
    pub subpath: Option<String>,
}

/// Result of listing skills from a repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoSkillList {
    pub repo_url: String,
    pub count: usize,
    pub skills: Vec<RepoSkillInfo>,
}

/// Result of installing a skill directly from a GitHub repo.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct RepoInstallResult {
    pub installed: bool,
    pub name: String,
    pub dcc: String,
    pub repo_url: String,
    pub path: String,
    pub skill_search_path: String,
    pub skill_subpath: Option<String>,
    pub description: Option<String>,
}

// ── helpers ───────────────────────────────────────────────────────────────────

/// Check whether `entry` targets the given DCC type (case-insensitive).
pub fn entry_targets_dcc(entry: &CatalogEntry, dcc: &str) -> bool {
    let dccs = entry_dcc_ids(entry);
    if is_shared_dcc_request(dcc) {
        return dccs.len() > 1 || dccs.iter().any(|value| is_host_neutral_dcc(value));
    }
    dccs.iter()
        .any(|value| is_host_neutral_dcc(value) || value.eq_ignore_ascii_case(dcc))
}

/// Return the declared DCC ids in declaration order, normalized and deduplicated.
pub fn entry_dcc_ids(entry: &CatalogEntry) -> Vec<String> {
    let mut ids = Vec::new();
    for dcc in &entry.dcc {
        let normalized = dcc.to_lowercase();
        if !ids.iter().any(|known| known == &normalized) {
            ids.push(normalized);
        }
    }
    ids
}

/// True for the reserved host-neutral marketplace directory name.
pub fn is_host_neutral_dcc(dcc: &str) -> bool {
    is_shared_dcc_request(dcc)
}

/// True for current and legacy spellings of the shared marketplace directory.
pub fn is_shared_install_dcc(dcc: &str) -> bool {
    is_shared_dcc_request(dcc)
}

/// True when a CLI request addresses the shared host-neutral directory.
pub fn is_shared_dcc_request(dcc: &str) -> bool {
    SHARED_DCC_REQUESTS
        .iter()
        .any(|request| dcc.eq_ignore_ascii_case(request))
}

/// True when an installed package is visible to the requested DCC.
pub fn package_serves_dcc(package: &InstalledMarketplacePackage, dcc: &str) -> bool {
    is_shared_install_dcc(&package.dcc) || package.dcc.eq_ignore_ascii_case(dcc)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
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
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        };
        assert!(entry_targets_dcc(&entry, "Maya"));
        assert!(entry_targets_dcc(&entry, "BLENDER"));
        assert!(!entry_targets_dcc(&entry, "houdini"));
    }

    #[test]
    fn legacy_all_entry_is_host_neutral() {
        let entry = CatalogEntry {
            name: "legacy-shared".into(),
            description: "legacy shared entry".into(),
            dcc: vec!["all".into()],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            maintainer: None,
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        };

        assert!(is_host_neutral_dcc("all"));
        assert!(entry_targets_dcc(&entry, "blender"));
    }
}
