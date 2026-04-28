//! Relay-side configuration.

use std::time::Duration;

/// Configuration for a `dcc-mcp-tunnel-relay` instance.
///
/// Constructed once at process start and held by the relay state machine.
/// Subsequent PRs add knobs for the data-plane buffers, the frontend
/// listener bind addresses, and per-DCC routing policy; this skeleton
/// keeps only the values needed by the control-plane registry.
#[derive(Debug, Clone)]
pub struct RelayConfig {
    /// Shared HS256 secret used to validate inbound JWTs in
    /// [`dcc_mcp_tunnel_protocol::auth::validate`]. Must be at least 32
    /// bytes of entropy in production deployments.
    pub jwt_secret: Vec<u8>,

    /// Public hostname the relay advertises in [`RelayConfig::base_url`]
    /// when minting tunnel URLs. Logged into JWT `iss` for telemetry.
    pub public_host: String,

    /// Base URL — `wss://relay.example.com` — prepended to per-tunnel
    /// paths when the relay reports the assigned `public_url` in
    /// `RegisterAck`.
    pub base_url: String,

    /// Heartbeat-loss window before a tunnel is considered stale and
    /// evicted from the registry. Default: 30 s. Mirrors the `stale_*`
    /// knobs in `McpHttpConfig` so operators have one consistent metric.
    pub stale_timeout: Duration,

    /// Hard cap on simultaneously-registered tunnels. `0` disables the
    /// cap. Existing tunnels keep their slot when the cap is hit; new
    /// `Register` requests are rejected with
    /// [`dcc_mcp_tunnel_protocol::frame::ErrorCode::Internal`].
    pub max_tunnels: usize,
}

impl Default for RelayConfig {
    /// Test-friendly default with a placeholder secret. **Never** ship a
    /// production deployment with the default secret — generate one with
    /// `openssl rand -base64 48` and feed it in via the operator's
    /// preferred secret store.
    fn default() -> Self {
        Self {
            jwt_secret: b"insecure-dev-only-replace-before-prod".to_vec(),
            public_host: "localhost".into(),
            base_url: "ws://localhost:9870".into(),
            stale_timeout: Duration::from_secs(30),
            max_tunnels: 0,
        }
    }
}
