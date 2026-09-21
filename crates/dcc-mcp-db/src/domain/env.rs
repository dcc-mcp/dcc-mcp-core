//! Canonical environment variable names for on-disk DCC-MCP state.

/// Shared file-backed service registry directory (`DCC_MCP_REGISTRY_DIR`).
pub const ENV_REGISTRY_DIR: &str = "DCC_MCP_REGISTRY_DIR";

/// Explicit override for the gateway admin SQLite file (traces, audits, custom skill paths).
pub const ENV_GATEWAY_ADMIN_DB: &str = "DCC_MCP_GATEWAY_ADMIN_DB";

/// Default filename inside the persistent application-state directory.
pub const GATEWAY_ADMIN_SQLITE_FILENAME: &str = "gateway_admin.sqlite";

/// Rolling file log directory for `dcc_mcp_logging` (`DCC_MCP_LOG_DIR`).
pub const ENV_DCC_MCP_LOG_DIR: &str = "DCC_MCP_LOG_DIR";

/// Repeat threshold at which a repeated escape-hatch script becomes a
/// skill-promotion candidate (#2297-A3). Unparsable values fall back to the
/// built-in default of 3.
pub const ENV_SCRIPT_PROMOTION_MIN_REPEATS: &str = "DCC_MCP_SCRIPT_PROMOTION_MIN_REPEATS";
