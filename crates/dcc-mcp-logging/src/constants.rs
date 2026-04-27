//! Logging-related constants — environment variable names and defaults.

/// Default log level when [`ENV_LOG_LEVEL`] is not set.
pub const DEFAULT_LOG_LEVEL: &str = "DEBUG";
/// Environment variable name for overriding the log level.
pub const ENV_LOG_LEVEL: &str = "MCP_LOG_LEVEL";

/// Environment variable toggling file-logging. Any non-empty, non-`0`/`false`
/// value enables it (defaults to disabled).
pub const ENV_LOG_FILE: &str = "DCC_MCP_LOG_FILE";
/// Environment variable overriding the log directory.
/// Falls back to `dcc_mcp_utils::filesystem::get_log_dir` when unset.
pub const ENV_LOG_DIR: &str = "DCC_MCP_LOG_DIR";
/// Environment variable overriding the maximum bytes per log file
/// before a rollover is triggered.
pub const ENV_LOG_MAX_SIZE: &str = "DCC_MCP_LOG_MAX_SIZE";
/// Environment variable overriding the retention count (how many rolled files to keep).
pub const ENV_LOG_MAX_FILES: &str = "DCC_MCP_LOG_MAX_FILES";
/// Environment variable overriding the rotation policy.
/// Accepts `size`, `daily`, `both` (case-insensitive).
pub const ENV_LOG_ROTATION: &str = "DCC_MCP_LOG_ROTATION";
/// Environment variable overriding the log file-name prefix.
pub const ENV_LOG_FILE_PREFIX: &str = "DCC_MCP_LOG_FILE_PREFIX";

/// Default maximum log file size in bytes before rotation (10 MiB).
pub const DEFAULT_LOG_MAX_SIZE: u64 = 10 * 1024 * 1024;
/// Default retention — keep this many rolled files in addition to the current one.
pub const DEFAULT_LOG_MAX_FILES: usize = 7;
/// Default log file-name prefix (the full filename is `<prefix>.<timestamp>.log`).
pub const DEFAULT_LOG_FILE_PREFIX: &str = "dcc-mcp";
/// Default rotation policy — `"both"` means rotate on size OR calendar-date change.
pub const DEFAULT_LOG_ROTATION: &str = "both";

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn env_var_names_are_stable() {
        assert_eq!(ENV_LOG_LEVEL, "MCP_LOG_LEVEL");
        assert_eq!(ENV_LOG_FILE, "DCC_MCP_LOG_FILE");
        assert_eq!(ENV_LOG_DIR, "DCC_MCP_LOG_DIR");
        assert_eq!(ENV_LOG_FILE_PREFIX, "DCC_MCP_LOG_FILE_PREFIX");
    }
}
