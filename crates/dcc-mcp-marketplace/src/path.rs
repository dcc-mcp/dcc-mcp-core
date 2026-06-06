//! Shared path resolution for marketplace root directory and sources config.
//!
//! Both the CLI and Gateway admin panel use these helpers so that the env-var and
//! home-directory fallback logic lives in one place.

use std::path::PathBuf;

use crate::error::MarketplaceError;

/// Environment variable that overrides the marketplace install root directory.
const ENV_MARKETPLACE_INSTALL_ROOT: &str = "DCC_MCP_MARKETPLACE_INSTALL_ROOT";
/// Environment variable that overrides the sources.json config file path.
const ENV_MARKETPLACE_SOURCES_FILE: &str = "DCC_MCP_MARKETPLACE_SOURCES_FILE";

/// Resolve the marketplace root directory.
///
/// Precedence: `DCC_MCP_MARKETPLACE_INSTALL_ROOT` env var → `$HOME/.dcc-mcp/marketplace`.
pub fn marketplace_root() -> Result<PathBuf, MarketplaceError> {
    if let Ok(value) = std::env::var(ENV_MARKETPLACE_INSTALL_ROOT)
        && !value.trim().is_empty()
    {
        return Ok(PathBuf::from(value));
    }
    let home = home_dir()
        .ok_or_else(|| MarketplaceError::ConfigPath("home directory is unavailable".into()))?;
    Ok(home.join(".dcc-mcp").join("marketplace"))
}

/// Resolve the path to `sources.json`.
///
/// Precedence: `DCC_MCP_MARKETPLACE_SOURCES_FILE` env var → `$HOME/.dcc-mcp/marketplace/sources.json`.
pub fn default_config_path() -> Result<PathBuf, MarketplaceError> {
    if let Ok(path) = std::env::var(ENV_MARKETPLACE_SOURCES_FILE)
        && !path.trim().is_empty()
    {
        return Ok(PathBuf::from(path));
    }
    let root = marketplace_root()?;
    Ok(root.join("sources.json"))
}

/// Resolve the current user's home directory.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("HOME")
        .or_else(|| std::env::var_os("USERPROFILE"))
        .map(PathBuf::from)
}

/// Fallback marketplace root — does not return an error, used when a non-fallible
/// resolution is needed (e.g. in Gateway where the service always builds).
pub fn marketplace_root_or_default() -> PathBuf {
    if let Ok(value) = std::env::var(ENV_MARKETPLACE_INSTALL_ROOT)
        && !value.trim().is_empty()
    {
        return PathBuf::from(value);
    }
    home_dir()
        .unwrap_or_else(|| PathBuf::from("."))
        .join(".dcc-mcp")
        .join("marketplace")
}
