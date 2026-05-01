use super::*;

/// How the gateway publishes backend-provided tools through MCP `tools/list`
/// (issue #652).
///
/// In multi-instance setups, fan-out of every live backend tool makes the
/// gateway's visible tool list grow linearly with instance count × skill
/// count, causing context blow-up on the client side. This enum lets the
/// operator bound the surface explicitly.
///
/// See the tracking issue [#657] for the REST-backed capability redesign
/// that `Slim` / `Rest` unlock.
///
/// [#657]: https://github.com/loonghao/dcc-mcp-core/issues/657
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum GatewayToolExposure {
    /// Current behavior: gateway meta-tools + skill-management tools +
    /// every live backend tool (Tier 1 + 2 + 3). Preserved as the default
    /// for compatibility during rollout.
    Full,
    /// Emit only gateway meta-tools + skill-management tools (Tier 1 + 2).
    /// Backend capabilities are expected to be discovered and invoked
    /// through dynamic `search_tools` / `describe_tool` / `call_tool`
    /// wrappers in a later phase of #657.
    Slim,
    /// Alias of [`Self::Full`] retained for forward compatibility with
    /// the documented `full | slim | both | rest` configuration surface.
    /// Behaves identically to `Full` today; may diverge once REST-backed
    /// dynamic tools land so that operators can run both static fan-out
    /// and dynamic wrappers side-by-side during migration.
    Both,
    /// Same bounded surface as [`Self::Slim`]; kept as a distinct variant
    /// so the gateway can signal "REST is the canonical capability API"
    /// in diagnostics and future routing decisions without another
    /// config migration.
    Rest,
}

impl GatewayToolExposure {
    /// Return `true` when the gateway should fan out to every live
    /// backend and publish each tool as an individual MCP tool.
    ///
    /// `Full` and `Both` both fan out today; `Slim` and `Rest` never do.
    pub const fn publishes_backend_tools(self) -> bool {
        matches!(self, Self::Full | Self::Both)
    }

    /// Human-readable token matching the documented config vocabulary
    /// (`full | slim | both | rest`). Used by diagnostics and the CLI.
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Full => "full",
            Self::Slim => "slim",
            Self::Both => "both",
            Self::Rest => "rest",
        }
    }
}

impl Default for GatewayToolExposure {
    /// Keep the pre-#652 behavior as the default so existing deployments
    /// see no change until they opt in.
    fn default() -> Self {
        Self::Full
    }
}

impl std::fmt::Display for GatewayToolExposure {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

impl std::str::FromStr for GatewayToolExposure {
    type Err = ParseGatewayToolExposureError;

    /// Parse the documented config tokens. Matching is case-insensitive
    /// so that CLI / env sources do not need to agree on casing; unknown
    /// values return a descriptive error instead of silently falling
    /// back to the default (which would mask operator typos).
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.trim().to_ascii_lowercase().as_str() {
            "full" => Ok(Self::Full),
            "slim" => Ok(Self::Slim),
            "both" => Ok(Self::Both),
            "rest" => Ok(Self::Rest),
            other => Err(ParseGatewayToolExposureError(other.to_string())),
        }
    }
}

/// Error returned by [`GatewayToolExposure::from_str`] for an unrecognised
/// token.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ParseGatewayToolExposureError(pub String);

impl std::fmt::Display for ParseGatewayToolExposureError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(
            f,
            "unknown gateway tool-exposure mode '{}' (expected one of: full, slim, both, rest)",
            self.0
        )
    }
}

impl std::error::Error for ParseGatewayToolExposureError {}

/// Configuration for the optional gateway.
pub struct GatewayConfig {
    /// Host to bind the gateway port on (default: `"127.0.0.1"`).
    pub host: String,
    /// Well-known port to compete for. `0` disables the gateway.
    pub gateway_port: u16,
    /// Seconds without heartbeat before an instance is considered stale.
    pub stale_timeout_secs: u64,
    /// Heartbeat interval in seconds. `0` disables the heartbeat task.
    pub heartbeat_secs: u64,
    /// Server name advertised in gateway `initialize` responses.
    pub server_name: String,
    /// Server version advertised in gateway `initialize` responses.
    pub server_version: String,
    /// Shared `FileRegistry` directory. `None` falls back to a temp dir.
    pub registry_dir: Option<PathBuf>,
    /// How many seconds a newer-version challenger waits for the old gateway
    /// to yield before giving up and running as a plain instance.
    ///
    /// Default: `120` seconds (12 × 10-second retry intervals).
    pub challenger_timeout_secs: u64,
    /// Per-backend request timeout (milliseconds) used for fan-out calls
    /// from the gateway to each live DCC instance. Default: `10_000`.
    /// Issue #314.
    pub backend_timeout_ms: u64,
    /// Longer timeout applied when the outbound `tools/call` is async-
    /// opted-in (issue #321). Default: `60_000`.
    pub async_dispatch_timeout_ms: u64,
    /// Gateway wait-for-terminal passthrough timeout (issue #321).
    /// Default: `600_000` (10 minutes).
    pub wait_terminal_timeout_ms: u64,
    /// TTL (seconds) for cached [`JobRoute`] entries in the gateway
    /// routing cache (issue #322). Routes older than this are evicted
    /// by a background GC task even if no terminal event was observed.
    /// Default: `86_400` (24 hours).
    ///
    /// [`JobRoute`]: super::sse_subscriber::JobRoute
    pub route_ttl_secs: u64,
    /// Per-session ceiling on concurrent live routes (issue #322). `0`
    /// disables the cap. Default: `1_000`.
    pub max_routes_per_session: u64,
    /// Allow instances with `dcc_type == "unknown"` to expose their tools
    /// via the gateway's `tools/list` and be reachable through `connect_to_dcc`.
    ///
    /// Default: `false`. Standalone `dcc-mcp-server` binaries that register
    /// with `dcc_type: "unknown"` should not leak tools into the gateway
    /// façade unless this is explicitly enabled for development (issue #555).
    pub allow_unknown_tools: bool,
    /// Adapter package version recorded on the `__gateway__` sentinel
    /// (e.g. `dcc_mcp_maya = "0.3.0"`). Used by the second tier of the
    /// election comparison (issue maya#137).
    pub adapter_version: Option<String>,
    /// DCC type the adapter is bound to (e.g. `"maya"`). Used by the
    /// third-tier real-DCC tiebreaker (issue maya#137).
    pub adapter_dcc: Option<String>,
    /// How the gateway publishes backend tools through MCP `tools/list`
    /// (issue #652).
    ///
    /// * [`GatewayToolExposure::Full`] — current behavior: every live
    ///   backend tool is visible on the gateway. Default for
    ///   compatibility during rollout.
    /// * [`GatewayToolExposure::Slim`] /
    ///   [`GatewayToolExposure::Rest`] — only gateway meta-tools and
    ///   skill-management tools are visible; backend capabilities must
    ///   be reached via the dynamic wrapper layer described in #657.
    /// * [`GatewayToolExposure::Both`] — currently an alias of `Full`,
    ///   reserved for the transition window once dynamic wrapper tools
    ///   land so operators can run both modes side-by-side.
    pub tool_exposure: GatewayToolExposure,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            host: "127.0.0.1".to_string(),
            gateway_port: 9765,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            server_name: "dcc-mcp-gateway".to_string(),
            server_version: env!("CARGO_PKG_VERSION").to_string(),
            registry_dir: None,
            challenger_timeout_secs: 120,
            backend_timeout_ms: 10_000,
            async_dispatch_timeout_ms: 60_000,
            wait_terminal_timeout_ms: 600_000,
            route_ttl_secs: 60 * 60 * 24,
            max_routes_per_session: 1_000,
            allow_unknown_tools: false,
            adapter_version: None,
            adapter_dcc: None,
            tool_exposure: GatewayToolExposure::Full,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // ── Parser: happy paths ──────────────────────────────────────────────
    //
    // The exposure token is surfaced through CLI, env var, and Python, so
    // every variant must parse without ambiguity. These tests also pin the
    // canonical lowercase spelling the rest of the codebase relies on.

    #[test]
    fn parses_every_canonical_token() {
        assert_eq!(
            "full".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Full
        );
        assert_eq!(
            "slim".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Slim
        );
        assert_eq!(
            "both".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Both
        );
        assert_eq!(
            "rest".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Rest
        );
    }

    #[test]
    fn parser_is_case_insensitive_and_trims_whitespace() {
        // CLI / env sources vary in casing discipline; we accept all of
        // them and normalise, but still reject typos loudly (see the
        // next test).
        assert_eq!(
            "  FULL ".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Full
        );
        assert_eq!(
            "Slim".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Slim
        );
        assert_eq!(
            "REST".parse::<GatewayToolExposure>().unwrap(),
            GatewayToolExposure::Rest
        );
    }

    // ── Parser: error paths ──────────────────────────────────────────────
    //
    // A typo'd exposure mode must surface at startup, not mask itself as
    // the default — that is the whole reason `from_str` returns `Result`
    // instead of falling back via `unwrap_or_default`.

    #[test]
    fn parser_rejects_unknown_token_with_descriptive_error() {
        let err = "ful".parse::<GatewayToolExposure>().unwrap_err();
        let msg = err.to_string();
        assert!(msg.contains("ful"), "error must echo the bad token: {msg}");
        assert!(
            msg.contains("full") && msg.contains("slim") && msg.contains("rest"),
            "error must enumerate the accepted tokens: {msg}"
        );
    }

    #[test]
    fn parser_rejects_empty_string() {
        // Empty input is a distinct failure mode (env var set but blank)
        // and must still produce a named error rather than silently
        // defaulting to Full.
        assert!("".parse::<GatewayToolExposure>().is_err());
        assert!("   ".parse::<GatewayToolExposure>().is_err());
    }

    // ── Semantics: publishes_backend_tools ───────────────────────────────
    //
    // This predicate is the single branch point in `aggregate_tools_list`
    // (#652). The test freezes the expected truth table so nobody can
    // quietly flip Slim/Rest back into a fan-out mode.

    #[test]
    fn publishes_backend_tools_truth_table_is_stable() {
        assert!(GatewayToolExposure::Full.publishes_backend_tools());
        assert!(GatewayToolExposure::Both.publishes_backend_tools());
        assert!(!GatewayToolExposure::Slim.publishes_backend_tools());
        assert!(!GatewayToolExposure::Rest.publishes_backend_tools());
    }

    #[test]
    fn as_str_matches_parser_vocabulary() {
        // Round-trip: `as_str` output must parse back to the same variant
        // so diagnostics / CLI help / docs never drift.
        for mode in [
            GatewayToolExposure::Full,
            GatewayToolExposure::Slim,
            GatewayToolExposure::Both,
            GatewayToolExposure::Rest,
        ] {
            let round_trip: GatewayToolExposure = mode.as_str().parse().unwrap();
            assert_eq!(round_trip, mode, "round-trip broke for {mode}");
        }
    }

    #[test]
    fn display_impl_uses_lowercase_token() {
        // Several downstream callers (diagnostics JSON, log lines) rely
        // on the Display impl. Lock the format so they stay readable.
        assert_eq!(format!("{}", GatewayToolExposure::Full), "full");
        assert_eq!(format!("{}", GatewayToolExposure::Slim), "slim");
        assert_eq!(format!("{}", GatewayToolExposure::Both), "both");
        assert_eq!(format!("{}", GatewayToolExposure::Rest), "rest");
    }

    #[test]
    fn default_is_full_for_compatibility() {
        // Pre-#652 behaviour must remain the default so existing
        // deployments see no change until they explicitly opt into a
        // bounded mode.
        assert_eq!(
            GatewayToolExposure::default(),
            GatewayToolExposure::Full,
            "changing the default without a migration would regress every \
             existing gateway deployment; guard it with this test."
        );
    }

    #[test]
    fn gateway_config_default_carries_full_exposure() {
        // `GatewayConfig::default()` is used by tests and `..Default::default()`
        // struct updates in the runner / standalone server. Keep the
        // field in lockstep with `GatewayToolExposure::default()`.
        let cfg = GatewayConfig::default();
        assert_eq!(cfg.tool_exposure, GatewayToolExposure::Full);
    }
}
