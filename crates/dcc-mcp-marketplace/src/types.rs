//! Shared marketplace domain types.
//!
//! These are the canonical types used by both the CLI and the Gateway admin
//! panel. The Gateway maps them to HTTP response types in its own adapter layer.

use dcc_mcp_catalog::{
    CatalogComponent, CatalogEntry, CatalogPackageFormat, CatalogTarget, CatalogTargetKind,
};
use serde::{Deserialize, Serialize};

// ── source ────────────────────────────────────────────────────────────────────

/// Canonical URL for the official dcc-mcp/marketplace catalog.
pub const OFFICIAL_MARKETPLACE_SOURCE: &str =
    "https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.json";
/// Detached Sigstore bundle for [`OFFICIAL_MARKETPLACE_SOURCE`].
pub const OFFICIAL_MARKETPLACE_ATTESTATION: &str =
    "https://raw.githubusercontent.com/dcc-mcp/marketplace/main/marketplace.sigstore.json";

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
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CatalogTarget>,
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
    pub target: CatalogTarget,
    pub version: Option<String>,
    pub path: String,
    pub skill_search_path: String,
    pub source: MarketplaceSource,
    pub entry: CatalogEntry,
    pub install_type: String,
    /// Immutable git revision that was actually installed, when the source supplies one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub resolved_commit: Option<String>,
    pub reload_required: bool,
    pub activation: MarketplaceActivation,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceUninstallResult {
    pub uninstalled: bool,
    pub name: String,
    pub dcc: String,
    pub target: CatalogTarget,
    pub path: String,
    pub removed_state: bool,
    pub removed_files: bool,
    pub reload_required: bool,
    pub activation: MarketplaceActivation,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum MarketplaceActivation {
    None,
    SkillReload,
    Restart,
}

// ── installed state ──────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(try_from = "InstalledMarketplacePackageWire")]
pub struct InstalledMarketplacePackage {
    pub name: String,
    pub dcc: String,
    pub target: CatalogTarget,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub components: Vec<CatalogComponent>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package_format: Option<CatalogPackageFormat>,
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

/// Persistence DTO that keeps DCC-only ledgers written before `target` compatible.
#[derive(Debug, Clone, Deserialize)]
struct InstalledMarketplacePackageWire {
    name: String,
    dcc: String,
    #[serde(default)]
    target: Option<CatalogTarget>,
    #[serde(default)]
    components: Vec<CatalogComponent>,
    #[serde(default)]
    package_format: Option<CatalogPackageFormat>,
    version: Option<String>,
    path: String,
    source_name: String,
    source_url: String,
    install_type: String,
    install_url: Option<String>,
    install_ref: Option<String>,
    #[serde(default)]
    resolved_commit: Option<String>,
    installed_at_ms: u128,
}

impl TryFrom<InstalledMarketplacePackageWire> for InstalledMarketplacePackage {
    type Error = String;

    fn try_from(value: InstalledMarketplacePackageWire) -> Result<Self, Self::Error> {
        let target = match value.target {
            Some(target) => target,
            None => {
                let legacy_dcc = value.dcc.trim();
                if legacy_dcc.is_empty() {
                    return Err("missing field `target` and legacy `dcc` is empty".into());
                }
                CatalogTarget {
                    kind: CatalogTargetKind::Dcc,
                    id: legacy_dcc.to_ascii_lowercase(),
                }
            }
        };
        Ok(Self {
            name: value.name,
            dcc: value.dcc,
            target,
            components: value.components,
            package_format: value.package_format,
            version: value.version,
            path: value.path,
            source_name: value.source_name,
            source_url: value.source_url,
            install_type: value.install_type,
            install_url: value.install_url,
            install_ref: value.install_ref,
            resolved_commit: value.resolved_commit,
            installed_at_ms: value.installed_at_ms,
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, Default, PartialEq, Eq)]
pub struct MarketplaceInstalledState {
    #[serde(default)]
    pub packages: Vec<InstalledMarketplacePackage>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct MarketplaceInstalledList {
    pub dcc: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target: Option<CatalogTarget>,
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
///
/// `any` is the host-neutral wildcard used by Skills that can be loaded into a
/// concrete adapter without owning a standalone DCC runtime.
pub fn entry_targets_dcc(entry: &CatalogEntry, dcc: &str) -> bool {
    entry_targets(entry).iter().any(|target| {
        target.kind == CatalogTargetKind::Dcc
            && (target.id.eq_ignore_ascii_case("any") || target.id.eq_ignore_ascii_case(dcc))
    })
}

pub fn entry_targets(entry: &CatalogEntry) -> Vec<CatalogTarget> {
    if !entry.targets.is_empty() {
        return entry.targets.clone();
    }
    entry
        .dcc
        .iter()
        .map(|id| CatalogTarget {
            kind: CatalogTargetKind::Dcc,
            id: id.clone(),
        })
        .collect()
}

pub fn parse_target(value: &str) -> Result<CatalogTarget, MarketplaceTargetParseError> {
    let (kind, id) = value.split_once(':').ok_or(MarketplaceTargetParseError)?;
    if id.trim().is_empty() {
        return Err(MarketplaceTargetParseError);
    }
    let kind = match kind {
        "dcc" => CatalogTargetKind::Dcc,
        "application" => CatalogTargetKind::Application,
        "game" => CatalogTargetKind::Game,
        "web" => CatalogTargetKind::Web,
        _ => return Err(MarketplaceTargetParseError),
    };
    Ok(CatalogTarget {
        kind,
        id: id.to_string(),
    })
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MarketplaceTargetParseError;

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
            targets: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            package: None,
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
    fn entry_targets_dcc_treats_any_as_host_neutral() {
        let entry = CatalogEntry {
            name: "host-neutral".into(),
            description: "desc".into(),
            dcc: vec!["ANY".into()],
            targets: vec![],
            url: None,
            tags: vec![],
            version: None,
            min_core_version: None,
            install: None,
            package: None,
            maintainer: None,
            category: None,
            policy: None,
            requires: None,
            icon: None,
            showcase: None,
        };

        assert!(entry_targets_dcc(&entry, "maya"));
        assert!(entry_targets_dcc(&entry, "Blender"));
        assert!(entry_targets_dcc(&entry, "custom-host"));
    }
}
