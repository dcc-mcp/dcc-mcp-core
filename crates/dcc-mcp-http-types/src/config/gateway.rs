use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use dcc_mcp_gateway_core::policy::GatewayPolicy;

/// Configured tunnel relay source for gateway discovery.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct RelaySourceConfig {
    /// Private/admin URL whose `/tunnels` endpoint returns live tunnel rows.
    pub admin_url: String,
    /// Public HTTP(S) frontend base URL that proxies `/tunnel/{id}/...`.
    pub public_base_url: String,
    /// Optional poll interval in seconds. Defaults to the gateway runtime.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub poll_interval_secs: Option<u64>,
}

/// Gateway election, routing, and discovery configuration.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct GatewayConfig {
    /// Gateway port to compete for. First process to bind wins the gateway
    /// and starts serving `/instances`, `/mcp`, `/mcp/{id}`, `/mcp/dcc/{type}`.
    /// `0` disables the gateway entirely. Default: 0 (disabled).
    pub gateway_port: u16,

    /// Optional second gateway listener for remote/LAN clients.
    ///
    /// This listener does not participate in gateway election; it is opened
    /// only by the process that wins `gateway_port`.
    pub remote_host: Option<String>,

    /// Optional second gateway port for remote/LAN clients. `0` disables it.
    pub remote_gateway_port: u16,

    /// Shared `FileRegistry` directory. `None` uses a system temp dir.
    pub registry_dir: Option<PathBuf>,

    /// Seconds without a heartbeat before an instance is considered stale.
    /// Default: 30.
    pub stale_timeout_secs: u64,

    /// Heartbeat interval in seconds. `0` disables the heartbeat task.
    /// Default: 5.
    pub heartbeat_secs: u64,

    /// Per-backend request timeout (milliseconds) used by the gateway when
    /// fanning out `tools/list` / `tools/call` to live DCC instances.
    ///
    /// Default: `120_000` (120 seconds / 2 minutes). DCC scene operations
    /// (mesh import, simulation bake, render, complex keyframe setup) regularly
    /// take tens of seconds. The previous default of 10 s caused the gateway to
    /// cancel legitimate tool calls while the backend was still working, logging
    /// "tool call cancelled cooperatively" on the DCC side at exactly 10 s.
    ///
    /// For truly long-running operations (renders, heavy simulations) prefer
    /// async dispatch (`_meta.dcc.async = true`) which returns a `job_id`
    /// immediately and lets the client poll via `jobs_get_status`.
    ///
    /// Only the gateway fan-out uses this value — per-instance servers
    /// bound to a DCC execute inline and are governed by
    /// [`ServerConfig::request_timeout_ms`] instead. Fixes issue #314.
    pub backend_timeout_ms: u64,

    /// Per-backend request timeout (milliseconds) applied by the gateway
    /// when the client has opted into **async dispatch** (issue #321).
    pub gateway_async_dispatch_timeout_ms: u64,

    /// Gateway timeout (milliseconds) for the opt-in wait-for-terminal
    /// response-stitching mode (issue #321).
    pub gateway_wait_terminal_timeout_ms: u64,

    /// TTL (seconds) for the gateway's per-job routing cache (issue #322).
    pub gateway_route_ttl_secs: u64,

    /// Per-session ceiling on concurrent live routes in the gateway
    /// routing cache (issue #322). `0` disables the cap.
    pub gateway_max_routes_per_session: u64,

    /// Adapter package version (e.g. `dcc_mcp_maya = "0.3.0"`) recorded
    /// on the `__gateway__` sentinel and used as the second tier of the
    /// version-aware gateway election (issue maya#137).
    pub adapter_version: Option<String>,

    /// DCC type the adapter is bound to (e.g. `"maya"`). Drives the
    /// third-tier "real DCC over generic standalone" tiebreaker in
    /// gateway election (issue maya#137).
    pub adapter_dcc: Option<String>,

    /// Human-readable identity for this gateway candidate. The elected
    /// gateway writes it to the `__gateway__` sentinel so operators can see
    /// which process owns the gateway port.
    pub gateway_name: Option<String>,

    /// Allow instances with `dcc_type == "unknown"` to expose their tools
    /// via the gateway (issue #555).
    ///
    /// Default: `false`. When `false`, the gateway's `tools/list` and
    /// `connect_to_dcc` ignore any instance whose `dcc_type` is
    /// `"unknown"` (case-insensitive). Set to `true` only for development
    /// or when intentionally running a standalone server without a real DCC.
    pub allow_unknown_tools: bool,

    /// Discover LAN-local DCC MCP endpoints via mDNS/DNS-SD.
    ///
    /// Default: `false`. Embedders must also build the runtime with the
    /// `mdns` feature; otherwise this value is ignored by the gateway bridge.
    pub discover_mdns: bool,

    /// Discover DCC MCP endpoints registered behind tunnel relays.
    ///
    /// Default: empty. Each configured relay is polled through its admin URL
    /// and routed through its HTTP(S) frontend URL.
    pub relay_sources: Vec<RelaySourceConfig>,

    /// Gateway capability policy applied before dynamic tools reach clients
    /// and before routed backend calls execute.
    ///
    /// Default is unrestricted. Configure read-only mode plus DCC, skill, and
    /// canonical `tool_slug` allowlists for locked-down deployments.
    pub policy: GatewayPolicy,

    /// Enable the read-only gateway admin dashboard.
    ///
    /// Default: `true`. Only the elected gateway process mounts this path,
    /// so a multi-instance process group still exposes a single admin UI.
    pub admin_enabled: bool,

    /// URL prefix for the admin dashboard. Default: `"/admin"`.
    pub admin_path: String,
}

impl Default for GatewayConfig {
    fn default() -> Self {
        Self {
            gateway_port: 0,
            remote_host: None,
            remote_gateway_port: 0,
            registry_dir: None,
            stale_timeout_secs: 30,
            heartbeat_secs: 5,
            backend_timeout_ms: 120_000,
            gateway_async_dispatch_timeout_ms: 60_000,
            gateway_wait_terminal_timeout_ms: 600_000,
            gateway_route_ttl_secs: 60 * 60 * 24,
            gateway_max_routes_per_session: 1_000,
            adapter_version: None,
            adapter_dcc: None,
            gateway_name: None,
            allow_unknown_tools: false,
            discover_mdns: false,
            relay_sources: Vec::new(),
            policy: GatewayPolicy::default(),
            admin_enabled: true,
            admin_path: "/admin".to_string(),
        }
    }
}
