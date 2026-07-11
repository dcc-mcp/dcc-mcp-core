//! Marketplace source resolution and normalisation.

use dcc_mcp_catalog::CatalogInstall;

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

/// Resolve a repository-relative catalog asset at the entry's declared git ref.
///
/// Absolute HTTP(S) URLs remain supported for custom catalogs. Official entries
/// use relative paths so media and installable content share one immutable pin.
pub fn resolve_catalog_asset_url(
    asset: Option<&str>,
    install: Option<&CatalogInstall>,
) -> Option<String> {
    let asset = asset?;
    if asset.starts_with("http://") || asset.starts_with("https://") {
        return Some(asset.to_string());
    }
    if !asset.split('/').all(is_safe_path_segment) {
        return None;
    }

    let install = install?;
    if install.install_type != "git" {
        return None;
    }
    let repo_url = install.url.as_deref()?;
    let ref_ = install.ref_.as_deref()?;
    if ref_.len() != 40 || !ref_.bytes().all(|byte| byte.is_ascii_hexdigit()) {
        return None;
    }
    let repo = repo_url
        .strip_prefix("https://github.com/")?
        .trim_end_matches('/')
        .trim_end_matches(".git");
    let mut parts = repo.split('/');
    let owner = parts.next()?;
    let name = parts.next()?;
    if !is_safe_path_segment(owner) || !is_safe_path_segment(name) || parts.next().is_some() {
        return None;
    }

    Some(format!(
        "https://raw.githubusercontent.com/{owner}/{name}/{ref_}/{asset}"
    ))
}

fn is_safe_path_segment(segment: &str) -> bool {
    !segment.is_empty()
        && segment != "."
        && segment != ".."
        && segment
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-'))
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

    fn github_install() -> CatalogInstall {
        CatalogInstall {
            install_type: "git".into(),
            url: Some("https://github.com/dcc-mcp/dcc-example.git".into()),
            ref_: Some("0123456789012345678901234567890123456789".into()),
            sha256: None,
            skill_roots: None,
            pip_package: None,
            pip_extras: None,
            python_path: None,
            entry_point: None,
            instructions_url: None,
        }
    }

    #[test]
    fn resolves_catalog_asset_at_pinned_github_revision() {
        assert_eq!(
            resolve_catalog_asset_url(
                Some("docs/images/example-showcase.webp"),
                Some(&github_install())
            ),
            Some(
                "https://raw.githubusercontent.com/dcc-mcp/dcc-example/0123456789012345678901234567890123456789/docs/images/example-showcase.webp".into()
            )
        );
    }

    #[test]
    fn rejects_catalog_asset_parent_traversal() {
        assert_eq!(
            resolve_catalog_asset_url(Some("../secret.png"), Some(&github_install())),
            None
        );
    }

    #[test]
    fn rejects_catalog_asset_without_immutable_revision() {
        let mut install = github_install();
        for unsafe_ref in [
            "main",
            "refs/heads/main",
            "../0123456789012345678901234567890123456789",
        ] {
            install.ref_ = Some(unsafe_ref.into());
            assert_eq!(
                resolve_catalog_asset_url(Some("docs/showcase.webp"), Some(&install)),
                None
            );
        }
    }

    #[test]
    fn rejects_catalog_asset_separator_and_encoding_attacks() {
        for asset in [
            "/secret.png",
            "docs\\secret.png",
            "docs//secret.png",
            "docs/./secret.png",
            "docs/%2e%2e/secret.png",
            "docs/secret.png?raw=1",
        ] {
            assert_eq!(
                resolve_catalog_asset_url(Some(asset), Some(&github_install())),
                None
            );
        }
    }
}
