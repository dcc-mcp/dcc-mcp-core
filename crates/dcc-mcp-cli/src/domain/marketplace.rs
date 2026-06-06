use dcc_mcp_catalog::CatalogEntry;
use serde::{Deserialize, Serialize};

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

impl MarketplaceSourceOrigin {
    /// Priority value: lower = higher priority.
    /// Explicit(0) > Env(1) > Config(2) > Builtin(3)
    pub fn priority(&self) -> u8 {
        match self {
            Self::Explicit => 0,
            Self::Env => 1,
            Self::Config => 2,
            Self::Builtin => 3,
        }
    }
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

pub fn builtin_source() -> MarketplaceSource {
    MarketplaceSource {
        name: "dcc-mcp/marketplace".to_string(),
        url: OFFICIAL_MARKETPLACE_SOURCE.to_string(),
        origin: MarketplaceSourceOrigin::Builtin,
    }
}

pub fn normalise_source(raw: &str, origin: MarketplaceSourceOrigin) -> MarketplaceSource {
    let trimmed = raw.trim();
    let url = if trimmed.eq_ignore_ascii_case("dcc-mcp/marketplace") {
        OFFICIAL_MARKETPLACE_SOURCE.to_string()
    } else if looks_like_github_slug(trimmed) {
        format!("https://raw.githubusercontent.com/{trimmed}/main/marketplace.json")
    } else {
        trimmed.to_string()
    };
    let name = if trimmed.eq_ignore_ascii_case("dcc-mcp/marketplace") {
        "dcc-mcp/marketplace".to_string()
    } else if looks_like_github_slug(trimmed) {
        trimmed.to_string()
    } else {
        url.clone()
    };
    MarketplaceSource { name, url, origin }
}

pub fn entry_targets_dcc(entry: &CatalogEntry, dcc: &str) -> bool {
    entry
        .dcc
        .iter()
        .any(|value| value.eq_ignore_ascii_case(dcc))
}

fn looks_like_github_slug(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty() && !repo.is_empty() && !value.contains("://") && !value.contains('\\')
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn normalises_official_slug() {
        let source = normalise_source("dcc-mcp/marketplace", MarketplaceSourceOrigin::Explicit);
        assert_eq!(source.name, "dcc-mcp/marketplace");
        assert_eq!(source.url, OFFICIAL_MARKETPLACE_SOURCE);
    }

    #[test]
    fn normalises_github_slug_to_raw_marketplace_json() {
        let source = normalise_source("studio/private", MarketplaceSourceOrigin::Explicit);
        assert_eq!(
            source.url,
            "https://raw.githubusercontent.com/studio/private/main/marketplace.json"
        );
    }
}
