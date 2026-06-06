//! Marketplace application layer — thin wrapper around [`dcc_mcp_marketplace::MarketplaceService`].
//!
//! The CLI-specific concern is path resolution: the shared service takes explicit
//! root and config paths; this module auto-resolves them from environment
//! variables and home-directory defaults.

use std::path::PathBuf;

use dcc_mcp_marketplace::MarketplaceError;

const ENV_MARKETPLACE_SOURCES_FILE: &str = "DCC_MCP_MARKETPLACE_SOURCES_FILE";

// Re-export the shared service and error types.
pub use dcc_mcp_marketplace::MarketplaceError as MarketplaceServiceError;
pub use dcc_mcp_marketplace::MarketplaceService;

/// Create a [`MarketplaceService`] with CLI-default paths resolved from the
/// environment and home directory.
pub fn new_service() -> Result<MarketplaceService, MarketplaceError> {
    let root = marketplace_root()?;
    let config_path = default_config_path()?;
    Ok(MarketplaceService::new(root).with_config_path(config_path))
}

// ── path resolution helpers ─────────────────────────────────────────────────

fn default_config_path() -> Result<PathBuf, MarketplaceError> {
    if let Ok(path) = std::env::var(ENV_MARKETPLACE_SOURCES_FILE)
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MarketplaceError::ConfigPath("home directory is unavailable".into()))?;
    Ok(PathBuf::from(home)
        .join(".dcc-mcp")
        .join("marketplace")
        .join("sources.json"))
}

fn marketplace_root() -> Result<PathBuf, MarketplaceError> {
    if let Ok(value) = std::env::var("DCC_MCP_MARKETPLACE_INSTALL_ROOT")
        && !value.trim().is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    let home = std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .ok_or_else(|| MarketplaceError::ConfigPath("home directory is unavailable".into()))?;
    Ok(PathBuf::from(home).join(".dcc-mcp").join("marketplace"))
}

/// Test-only constructor with a custom config path.
#[cfg(test)]
pub fn service_with_config_path(config_path: PathBuf) -> MarketplaceService {
    let root = config_path
        .parent()
        .map(PathBuf::from)
        .unwrap_or_else(|| PathBuf::from("."));
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
