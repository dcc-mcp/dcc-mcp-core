//! Marketplace HTTP contracts owned by the admin API.

use dcc_mcp_catalog::CatalogRequirements;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MarketplaceEntryResponse {
    pub name: String,
    pub description: String,
    pub dcc: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    pub tags: Vec<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub min_core_version: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub maintainer: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub requires: Option<CatalogRequirements>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub source_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub showcase: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install: Option<InstallMetadataResponse>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallMetadataResponse {
    #[serde(rename = "type")]
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", rename = "ref")]
    pub ref_: Option<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstalledPackageResponse {
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub source_name: String,
    pub source_url: String,
    pub install_type: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_url: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub install_ref: Option<String>,
    pub installed_at_ms: u128,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct InstallResultResponse {
    pub installed: bool,
    pub name: String,
    pub dcc: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    pub path: String,
    pub skill_search_path: String,
    pub install_type: String,
    pub reload_required: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct UninstallResultResponse {
    pub uninstalled: bool,
    pub name: String,
    pub dcc: String,
    pub path: String,
    pub removed_state: bool,
    pub removed_files: bool,
    pub reload_required: bool,
}

#[derive(Debug, Deserialize)]
pub struct InstallRequestBody {
    pub name: String,
    pub dcc: String,
    #[serde(default)]
    pub source: Option<String>,
    #[serde(default)]
    pub force: bool,
}

#[derive(Debug, Deserialize)]
pub struct UninstallRequestBody {
    pub name: String,
    pub dcc: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct MarketplaceSourceResponse {
    pub name: String,
    pub url: String,
    pub origin: String,
}

#[derive(Debug, Deserialize)]
pub struct AddSourceRequest {
    pub source: String,
}

#[derive(Debug, Clone, Serialize)]
pub struct OutdatedPackageResponse {
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

#[derive(Debug, Deserialize)]
pub struct UpdateRequest {
    pub name: Option<String>,
    pub dcc: Option<String>,
    #[serde(default)]
    pub all: bool,
}

#[derive(Debug, Serialize)]
pub struct UpdateResultItem {
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

#[derive(Debug, Deserialize)]
pub struct OutdatedQueryParams {
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub dcc: Option<String>,
}

/// Resolve an icon path against a raw GitHub catalog URL when possible.
#[must_use]
pub fn resolve_marketplace_icon_url(
    icon: Option<&str>,
    source_url: Option<&str>,
) -> Option<String> {
    let icon = icon?;
    if icon.starts_with("http://") || icon.starts_with("https://") {
        return Some(icon.to_string());
    }
    let source_url = source_url?;
    if source_url.contains("raw.githubusercontent.com")
        && let Some((base, _)) = source_url.rsplit_once('/')
    {
        return Some(format!("{base}/{}", icon.trim_start_matches('/')));
    }
    Some(icon.to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn resolves_relative_icon_against_raw_github_catalog() {
        let source = "https://raw.githubusercontent.com/dcc-mcp/example/main/marketplace.json";
        assert_eq!(
            resolve_marketplace_icon_url(Some("icon.png"), Some(source)),
            Some("https://raw.githubusercontent.com/dcc-mcp/example/main/icon.png".to_string())
        );
    }

    #[test]
    fn preserves_absolute_and_non_github_icon_urls() {
        let absolute = "https://cdn.example.com/icon.png";
        assert_eq!(
            resolve_marketplace_icon_url(Some(absolute), None),
            Some(absolute.to_string())
        );
        assert_eq!(
            resolve_marketplace_icon_url(
                Some("icon.png"),
                Some("https://example.com/catalog.json")
            ),
            Some("icon.png".to_string())
        );
    }
}
