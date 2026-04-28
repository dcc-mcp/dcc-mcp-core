//! Agent-side configuration.

use std::time::Duration;

/// Reconnect policy used when the relay leg drops.
///
/// The agent always retries — there is no `Never` variant — because the
/// only sensible behaviour for a sidecar is to keep the tunnel alive
/// until the operator explicitly tears it down.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ReconnectPolicy {
    /// Constant delay between attempts. Easiest to reason about; suitable
    /// for tests and well-connected LAN deployments.
    Constant {
        /// Wait this long between attempts.
        delay: Duration,
    },

    /// Exponential back-off, doubling each attempt up to a ceiling. The
    /// first retry happens after `initial`; the n-th waits
    /// `min(initial * 2^(n-1), max)`. After 60 minutes of unbroken
    /// failure the backoff resets to `initial`.
    Exponential {
        /// Delay before the first retry.
        initial: Duration,
        /// Hard ceiling on the delay between attempts.
        max: Duration,
    },
}

impl Default for ReconnectPolicy {
    fn default() -> Self {
        Self::Exponential {
            initial: Duration::from_secs(2),
            max: Duration::from_secs(60),
        }
    }
}

/// Configuration for a `dcc-mcp-tunnel-agent` instance.
#[derive(Debug, Clone)]
pub struct AgentConfig {
    /// `wss://relay.example.com` — the relay's WebSocket entrypoint. The
    /// agent appends the registration path itself.
    pub relay_url: String,

    /// Bearer JWT minted by [`dcc_mcp_tunnel_protocol::auth::issue`].
    /// Embedded into the `RegisterRequest` frame.
    pub token: String,

    /// DCC tag this agent identifies with (`"maya"`, `"houdini"`, …).
    /// Must be in the JWT's `allowed_dcc` list, otherwise the relay
    /// rejects the registration with `DccNotAllowed`.
    pub dcc: String,

    /// Capability tags forwarded to remote clients via the relay.
    pub capabilities: Vec<String>,

    /// Build identifier reported to the relay; surfaced in `/tunnels`
    /// listings only.
    pub agent_version: String,

    /// `127.0.0.1:8765` — local MCP HTTP server the agent bridges to.
    /// On each `OpenSession` from the relay, the agent opens a fresh
    /// connection here.
    pub local_target: String,

    /// Cadence at which the agent emits `Frame::Heartbeat`. The relay
    /// evicts tunnels silent longer than its own `stale_timeout`, so
    /// keep this comfortably under that window.
    pub heartbeat_interval: Duration,

    /// What to do when the relay leg drops.
    pub reconnect: ReconnectPolicy,
}

impl AgentConfig {
    /// Sensible defaults for everything except the four fields the
    /// operator must fill in (`relay_url`, `token`, `dcc`, `local_target`).
    pub fn new(
        relay_url: impl Into<String>,
        token: impl Into<String>,
        dcc: impl Into<String>,
        local_target: impl Into<String>,
    ) -> Self {
        Self {
            relay_url: relay_url.into(),
            token: token.into(),
            dcc: dcc.into(),
            capabilities: Vec::new(),
            agent_version: format!("dcc-mcp-tunnel-agent/{}", env!("CARGO_PKG_VERSION")),
            local_target: local_target.into(),
            heartbeat_interval: Duration::from_secs(10),
            reconnect: ReconnectPolicy::default(),
        }
    }
}
