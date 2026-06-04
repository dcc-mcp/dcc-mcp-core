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
