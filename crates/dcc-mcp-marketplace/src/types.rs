//! Shared marketplace domain types.
//!
//! These are the canonical types used by both the CLI and the Gateway admin
//! panel. The Gateway maps them to HTTP response types in its own adapter layer.

use dcc_mcp_catalog::CatalogEntry;
use serde::{Deserialize, Serialize};

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
    entry
        .dcc
        .iter()
        .any(|value| value.eq_ignore_ascii_case(dcc))
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
            icon: None,
        };
        assert!(entry_targets_dcc(&entry, "Maya"));
        assert!(entry_targets_dcc(&entry, "BLENDER"));
        assert!(!entry_targets_dcc(&entry, "houdini"));
    }
}
