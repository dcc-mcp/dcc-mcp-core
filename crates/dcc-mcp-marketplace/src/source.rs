//! Marketplace source resolution and normalisation.

use crate::types::{MarketplaceSource, MarketplaceSourceOrigin, OFFICIAL_MARKETPLACE_SOURCE};

/// Build a built-in source pointing to the official marketplace.
pub fn builtin_source() -> MarketplaceSource {
    MarketplaceSource {
        name: "dcc-mcp/marketplace".to_string(),
        url: OFFICIAL_MARKETPLACE_SOURCE.to_string(),
        origin: MarketplaceSourceOrigin::Builtin,
    }
}

/// Normalise a raw source string (slug, URL, or path) into a [`MarketplaceSource`].
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

fn looks_like_github_slug(value: &str) -> bool {
    let Some((owner, repo)) = value.split_once('/') else {
        return false;
    };
    !owner.is_empty() && !repo.is_empty() && !value.contains("://") && !value.contains('\\')
}

/// Deduplicate sources by URL, keeping the first occurrence.
pub fn dedupe_sources(sources: Vec<MarketplaceSource>) -> Vec<MarketplaceSource> {
    let mut seen = std::collections::HashSet::new();
    let mut result = Vec::new();
    for source in sources {
        if seen.insert(source.url.clone()) {
            result.push(source);
        }
    }
    result
}

// ── tests ─────────────────────────────────────────────────────────────────────

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

    #[test]
    fn normalises_url_passthrough() {
        let source = normalise_source(
            "https://example.com/catalog.json",
            MarketplaceSourceOrigin::Explicit,
        );
        assert_eq!(source.url, "https://example.com/catalog.json");
    }

    #[test]
    fn normalises_absolute_path_passthrough() {
        let source = normalise_source("/tmp/catalog.json", MarketplaceSourceOrigin::Explicit);
        assert_eq!(source.url, "/tmp/catalog.json");
    }
}
