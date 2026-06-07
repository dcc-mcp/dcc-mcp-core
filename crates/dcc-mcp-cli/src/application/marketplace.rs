//! Marketplace application layer — thin wrapper around [`dcc_mcp_marketplace::MarketplaceService`].
//!
//! Path resolution helpers now live in [`dcc_mcp_marketplace::path`].

use dcc_mcp_marketplace::MarketplaceError;

// Re-export the shared service, error types, and path helpers.
pub use dcc_mcp_marketplace::MarketplaceError as MarketplaceServiceError;
pub use dcc_mcp_marketplace::MarketplaceService;

/// Create a [`MarketplaceService`] with CLI-default paths resolved from the
/// environment and home directory.
pub fn new_service() -> Result<MarketplaceService, MarketplaceError> {
    let root = dcc_mcp_marketplace::marketplace_root()?;
    let config_path = dcc_mcp_marketplace::default_config_path()?;
    Ok(MarketplaceService::new(root).with_config_path(config_path))
}

/// Test-only constructor with a custom config path.
#[cfg(test)]
pub fn service_with_config_path(config_path: std::path::PathBuf) -> MarketplaceService {
    let root = config_path
        .parent()
        .map(std::path::PathBuf::from)
        .unwrap_or_else(|| std::path::PathBuf::from("."));
    MarketplaceService::new(root).with_config_path(config_path)
}

// ── tests ─────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn add_source_persists_to_config_file() {
        let dir = tempfile::tempdir().unwrap();
        let service = service_with_config_path(dir.path().join("sources.json"));

        let sources = service.add_source("studio/private").unwrap();

        assert!(sources.iter().any(|source| source.name == "studio/private"));
        let saved = std::fs::read_to_string(dir.path().join("sources.json")).unwrap();
        assert!(saved.contains("studio/private"));
    }
}
