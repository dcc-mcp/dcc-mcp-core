use serde::{Deserialize, Serialize};

// ── SessionConfig / GatewayConfig ─────────────────────────────────────────

/// Session lifecycle & tool-cache configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct SessionConfig {
    /// Idle session TTL in seconds. Sessions that have not received any
    /// request within this window are automatically evicted by a background
    /// task started by the HTTP server. Default: 3600 (1 hour).
    /// Set to 0 to disable automatic eviction.
    pub session_ttl_secs: u64,

    /// Enable connection-scoped tool-list caching (issue #438).
    ///
    /// When `true` (default), `tools/list` stores a per-session snapshot
    /// of the full tool list. On subsequent `tools/list` calls within the
    /// same session, if the registry generation has not changed (no skill
    /// load/unload, no group activation/deactivation), the cached
    /// snapshot is returned directly — avoiding redundant registry scans,
    /// bare-name resolution, and `McpTool` construction.
    ///
    /// The cache is automatically invalidated when:
    /// - A skill is loaded or unloaded
    /// - A tool group is activated or deactivated
    /// - The session is evicted (TTL expiry)
    /// - The client sends `tools/list` with `_meta.dcc.refresh = true`
    ///
    /// Set to `false` to disable caching (every `tools/list` call
    /// rebuilds the full list from scratch).
    pub enable_tool_cache: bool,
}

impl Default for SessionConfig {
    fn default() -> Self {
        Self {
            session_ttl_secs: 3_600,
            enable_tool_cache: true,
        }
    }
}
