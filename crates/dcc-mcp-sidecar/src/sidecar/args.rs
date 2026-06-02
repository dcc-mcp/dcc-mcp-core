use std::path::PathBuf;

use clap::Args;
use uuid::Uuid;

/// Reason the sidecar exited; used by the integration test and structured
/// logging.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// Parent DCC process (`--watch-pid`) was no longer alive.
    ParentDied,
    /// SIGINT / SIGTERM / `ctrl-c`.
    Signal,
}

/// CLI surface for the `sidecar` subcommand.
#[derive(Debug, Args)]
pub struct SidecarArgs {
    /// DCC identifier this sidecar serves (e.g. `maya`, `blender`, `houdini`).
    #[arg(long, value_name = "NAME")]
    pub dcc: String,

    /// RPC URI the sidecar uses to talk back to the live DCC.
    ///
    /// Examples:
    /// * `commandport://127.0.0.1:6000` - Maya `commandPort`
    /// * `qtserver://127.0.0.1:18765` - Qt in-process sidecar server
    /// * `ws://127.0.0.1:9000` - Photoshop UXP / Figma plugin
    /// * `stub://localhost` - tests only; connects but returns transport errors
    ///
    /// The scheme selects which registered `HostRpcClient` impl handles the
    /// connection. Unsupported schemes still leave a visible registry row with
    /// `dispatch_status=unavailable`; the sidecar may still publish a
    /// diagnostic MCP URL that returns structured transport errors instead of
    /// becoming routable.
    #[arg(long, value_name = "URI")]
    pub host_rpc: String,

    /// Parent DCC process PID. Sidecar exits cleanly when this PID is no
    /// longer alive.
    #[arg(long, value_name = "PID")]
    pub watch_pid: u32,

    /// `FileRegistry` directory. Defaults to platform-specific shared dir.
    #[arg(long, value_name = "PATH", env = "DCC_MCP_REGISTRY_DIR")]
    pub registry_dir: Option<PathBuf>,

    /// Instance UUID. Auto-generated if absent. Use this to make the
    /// sidecar's row stable across restarts (the parent DCC plugin can
    /// pin one so resume works).
    #[arg(long, value_name = "UUID")]
    pub instance_id: Option<Uuid>,

    /// Human-readable label for this sidecar (e.g. `Maya-Anim`).
    #[arg(long, value_name = "TEXT")]
    pub display_name: Option<String>,

    /// Adapter package version stamped onto the registry row
    /// (e.g. `dcc_mcp_maya = "0.3.0"`).
    #[arg(long, value_name = "SEMVER")]
    pub adapter_version: Option<String>,

    /// Seconds to wait for the initial ``HostRpcClient::connect`` to the
    /// DCC. Failure to connect within this budget is logged but does
    /// **not** abort the sidecar - the process keeps running so its
    /// FileRegistry row is visible and the PPID-watch can still detect
    /// parent death. The gateway sees a registered-but-disconnected
    /// backend and routes around it.
    #[arg(long, value_name = "SECS", default_value = "10")]
    pub connect_timeout_secs: u64,

    /// Test hook: allow `stub://` to publish dispatch-ready metadata.
    ///
    /// Production launchers must use a real host RPC scheme. Without this
    /// explicit opt-in, `stub://` remains a diagnostic listener so installers
    /// cannot mistake a test placeholder for a callable DCC dispatcher.
    #[arg(long, hide = true, default_value = "false")]
    pub allow_stub_dispatch_ready: bool,

    /// Override the polling interval for PPID watch (test hook).
    #[arg(long, value_name = "MS", hide = true)]
    pub ppid_poll_ms: Option<u64>,

    /// Well-known gateway port to ensure. ``0`` disables gateway auto-launch.
    ///
    /// Defaults to ``DCC_MCP_GATEWAY_PORT`` (9765). Per-DCC sidecars no longer
    /// compete for this port unless ``--legacy-gateway-election`` is set.
    #[arg(long, default_value = "9765", env = "DCC_MCP_GATEWAY_PORT")]
    pub gateway_port: u16,

    /// Disable auto-launching the machine-wide standalone gateway.
    #[arg(long, default_value = "false")]
    pub no_ensure_gateway: bool,

    /// Legacy mode: let this per-DCC sidecar compete for the gateway role.
    #[arg(long, env = "DCC_MCP_LEGACY_GATEWAY_ELECTION", default_value = "false")]
    pub legacy_gateway_election: bool,

    /// Legacy host/interface for the gateway listener (default ``127.0.0.1``).
    ///
    /// Prefer ``--gateway-host`` / ``DCC_MCP_GATEWAY_HOST`` for new launchers.
    #[arg(long, default_value = "127.0.0.1")]
    pub host: String,

    /// Gateway host/interface to bind. Use ``0.0.0.0`` to accept LAN clients.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST")]
    pub gateway_host: Option<String>,

    /// Human-readable gateway candidate name written to the `__gateway__`
    /// sentinel when this sidecar wins or challenges the gateway role.
    #[arg(long, env = "DCC_MCP_GATEWAY_NAME")]
    pub gateway_name: Option<String>,

    /// Remote/LAN gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "0.0.0.0")]
    pub gateway_remote_host: String,

    /// Remote/LAN gateway port. ``0`` disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    pub gateway_remote_port: u16,
}
