use std::path::PathBuf;

use clap::{Args as ClapArgs, Parser, Subcommand};

use crate::{capture, translate};

/// DCC-MCP subcommands.
#[derive(Debug, Subcommand)]
pub(crate) enum SubCmd {
    /// Run the default per-DCC server with first-wins auto-gateway.
    Auto(ServerArgs),
    /// Run a per-DCC server, optionally without participating in auto-gateway.
    Serve(ServeArgs),
    /// Bridge any stdio MCP server to HTTP/SSE/Streamable-HTTP.
    Translate(translate::TranslateArgs),
    /// Catalog commands (search or describe DCC-MCP adapters).
    Catalog {
        #[command(subcommand)]
        action: CatalogAction,
    },
    /// Out-of-process worker for crash-isolated DCC actions (RFC #998).
    ///
    /// Spawned by a DCC plugin/addon (`dcc-mcp-maya`, `dcc-mcp-blender`, ...)
    /// and supervised via `--watch-pid`. Exits cleanly when its parent
    /// DCC dies so we never leak stale workers.
    #[cfg(feature = "gateway-auto")]
    Sidecar(dcc_mcp_sidecar::SidecarArgs),
    /// Machine-wide gateway daemon. Per-DCC sidecars auto-launch this when needed.
    #[cfg(feature = "gateway-daemon")]
    Gateway(dcc_mcp_sidecar::gateway_daemon::GatewayArgs),
    /// Replay or diff gateway traffic capture files.
    Capture {
        #[command(subcommand)]
        action: capture::CaptureAction,
    },
}

/// DCC-MCP server CLI.
#[derive(Debug, Parser)]
#[command(
    name = "dcc-mcp-server",
    about,
    version,
    args_conflicts_with_subcommands = true
)]
pub(crate) struct Args {
    /// Optional subcommand. If omitted, runs as `auto` for backwards compatibility.
    #[command(subcommand)]
    pub(crate) command: Option<SubCmd>,

    #[command(flatten)]
    pub(crate) server: ServerArgs,
}

/// Shared per-DCC server flags for the implicit root mode and `auto` / `serve`.
#[derive(Debug, Clone, ClapArgs)]
pub(crate) struct ServerArgs {
    /// MCP Streamable HTTP server port. Default 0 = OS-assigned.
    #[arg(long, env = "DCC_MCP_MCP_PORT", default_value = "0")]
    pub(crate) mcp_port: u16,

    /// WebSocket bridge server port (for non-Python DCC plugins).
    #[arg(long, env = "DCC_MCP_WS_PORT", default_value = "9001")]
    pub(crate) ws_port: u16,

    /// Application type (e.g. "maya", "photoshop", "blender").
    #[arg(long, env = "DCC_MCP_APP", default_value = "")]
    pub(crate) app: String,

    /// Additional skill search paths (repeatable).
    #[arg(long, value_name = "PATH", num_args = 1..)]
    pub(crate) skill_paths: Vec<PathBuf>,

    /// Server name advertised to MCP clients.
    #[arg(long, env = "DCC_MCP_SERVER_NAME", default_value = "dcc-mcp-server")]
    pub(crate) server_name: String,

    /// Disable the WebSocket bridge server (MCP HTTP only).
    #[arg(long, default_value = "false")]
    pub(crate) no_bridge: bool,

    /// MCP server host to bind to.
    #[arg(long, default_value = "127.0.0.1")]
    pub(crate) host: String,

    /// Write the server process ID to this file while running.
    #[arg(long, value_name = "PATH")]
    pub(crate) pid_file: Option<PathBuf>,

    /// Overwrite an existing PID file even if it points at a live process.
    #[arg(long, default_value = "false")]
    pub(crate) force: bool,

    /// Seconds to wait for graceful shutdown before exiting.
    #[arg(long, env = "DCC_MCP_SHUTDOWN_TIMEOUT_SECS", default_value = "10")]
    pub(crate) shutdown_timeout_secs: u64,

    /// Gateway port to ensure/register with. Default startup ensures a
    /// standalone gateway daemon, then registers this process as a backend.
    /// 0 disables gateway ensure/election for this process.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    pub(crate) gateway_port: u16,

    /// Disable auto-launching the machine-wide standalone gateway before
    /// registering this per-DCC server.
    #[arg(long, default_value = "false")]
    pub(crate) no_ensure_gateway: bool,

    /// Legacy mode: let this per-DCC server compete for the gateway port
    /// instead of using the standalone gateway daemon as the local control
    /// plane.
    #[arg(long, env = "DCC_MCP_LEGACY_GATEWAY_ELECTION", default_value = "false")]
    pub(crate) legacy_gateway_election: bool,

    /// Gateway host/interface to bind. Defaults to the MCP `--host`.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST")]
    pub(crate) gateway_host: Option<String>,

    /// Human-readable gateway candidate name written to the `__gateway__`
    /// sentinel when this process wins or challenges the gateway role.
    #[arg(long, env = "DCC_MCP_GATEWAY_NAME")]
    pub(crate) gateway_name: Option<String>,

    /// Remote/LAN gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "0.0.0.0")]
    pub(crate) gateway_remote_host: String,

    /// Remote/LAN gateway port. 0 disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    pub(crate) gateway_remote_port: u16,

    /// Disable the read-only Admin UI on the elected gateway.
    #[arg(long, env = "DCC_MCP_NO_ADMIN", default_value = "false")]
    pub(crate) no_admin: bool,

    /// URL prefix for the read-only Admin UI.
    #[arg(long, env = "DCC_MCP_ADMIN_PATH", default_value = "/admin")]
    pub(crate) admin_path: String,

    /// Directory for the shared FileRegistry (auto-created if missing).
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    pub(crate) registry_dir: Option<String>,

    /// Seconds without a heartbeat before an instance is considered stale.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    pub(crate) stale_timeout_secs: u64,

    /// Application version (reported in registry, e.g. "2024.2").
    #[arg(long, env = "DCC_MCP_APP_VERSION")]
    pub(crate) app_version: Option<String>,

    /// Currently open scene file (reported in registry, improves routing).
    #[arg(long, env = "DCC_MCP_SCENE")]
    pub(crate) scene: Option<String>,

    /// Heartbeat interval in seconds for the registry. 0 = disabled.
    #[arg(long, env = "DCC_MCP_HEARTBEAT_INTERVAL", default_value = "5")]
    pub(crate) heartbeat_secs: u64,

    /// Reserved compatibility flag for older sidecar supervisors.
    #[arg(long, default_value = "30")]
    pub(crate) reconnect_timeout_secs: u64,

    /// Internal helper: watch a PID and remove its PID file after exit.
    #[arg(long, hide = true)]
    pub(crate) pid_cleanup_watch: Option<PathBuf>,

    /// Internal helper: PID to watch for `--pid-cleanup-watch`.
    #[arg(long, hide = true)]
    pub(crate) watch_pid: Option<u32>,

    /// Disable logging to rotating files. By default file logging is enabled
    /// unless this flag is passed.
    #[arg(long, env = "DCC_MCP_NO_LOG_FILE", default_value = "false")]
    pub(crate) no_log_file: bool,

    /// Directory for rotated log files. Defaults to the platform log dir
    /// (`dcc_mcp_paths::get_log_dir()`).
    #[arg(long, env = "DCC_MCP_LOG_DIR", value_name = "PATH")]
    pub(crate) log_dir: Option<PathBuf>,

    /// Maximum bytes per log file before a size-triggered rotation.
    #[arg(long, env = "DCC_MCP_LOG_MAX_SIZE", value_name = "BYTES")]
    pub(crate) log_max_size: Option<u64>,

    /// Number of **rolled** files to retain (current file excluded).
    #[arg(long, env = "DCC_MCP_LOG_MAX_FILES", value_name = "N")]
    pub(crate) log_max_files: Option<usize>,

    /// Rotation policy: `size`, `daily`, or `both`.
    #[arg(long, env = "DCC_MCP_LOG_ROTATION", value_name = "POLICY")]
    pub(crate) log_rotation: Option<String>,

    /// File-name prefix (full file is `<prefix>.<pid>.<YYYYMMDD>.log`).
    #[arg(long, env = "DCC_MCP_LOG_FILE_PREFIX", value_name = "PREFIX")]
    pub(crate) log_file_prefix: Option<String>,

    /// Log retention in days (0 = disable age pruning). Default: 7.
    #[arg(long, env = "DCC_MCP_LOG_RETENTION_DAYS", value_name = "DAYS")]
    pub(crate) log_retention_days: Option<u32>,

    /// Maximum total log directory size in MiB (0 = disable size pruning). Default: 100.
    #[arg(long, env = "DCC_MCP_LOG_MAX_TOTAL_SIZE_MB", value_name = "MB")]
    pub(crate) log_max_total_size_mb: Option<u32>,

    /// Advertise this per-DCC MCP endpoint on the LAN via mDNS/DNS-SD.
    #[cfg(feature = "mdns")]
    #[arg(long, env = "DCC_MCP_ADVERTISE_MDNS", default_value = "false")]
    pub(crate) advertise_mdns: bool,
}

/// Explicit per-DCC server mode.
#[derive(Debug, ClapArgs)]
pub(crate) struct ServeArgs {
    #[command(flatten)]
    pub(crate) server: ServerArgs,

    /// Run only the per-DCC MCP server and never compete for the gateway port.
    #[arg(long)]
    pub(crate) no_auto_gateway: bool,
}

impl ServeArgs {
    pub(crate) fn into_server_args(mut self) -> ServerArgs {
        if self.no_auto_gateway {
            self.server.gateway_port = 0;
        }
        self.server
    }
}

/// Catalog subcommand: query the public DCC-MCP catalog.
#[derive(Debug, Subcommand)]
pub(crate) enum CatalogAction {
    /// Search the catalog by keyword (name, description, DCC type, tags).
    Search {
        /// Keyword to search for. Omit to list all entries.
        #[arg(long, default_value = "")]
        query: String,
    },
    /// Show full details for a single catalog entry by exact name.
    Describe {
        /// Exact catalog entry name (e.g. dcc-mcp-maya-skills).
        #[arg(long)]
        name: String,
    },
}

#[cfg(test)]
mod tests {
    use clap::{Parser as _, error::ErrorKind};

    use super::{Args, SubCmd};

    #[test]
    fn no_subcommand_keeps_backwards_compatible_server_flags() {
        let parsed = Args::try_parse_from(["dcc-mcp-server", "--app", "maya"]).unwrap();

        assert!(parsed.command.is_none());
        assert_eq!(parsed.server.app, "maya");
        assert_eq!(parsed.server.gateway_port, 9765);
    }

    #[test]
    fn explicit_auto_uses_the_same_server_flag_surface() {
        let parsed = Args::try_parse_from(["dcc-mcp-server", "auto", "--app", "blender"]).unwrap();

        let Some(SubCmd::Auto(server)) = parsed.command else {
            panic!("expected auto subcommand");
        };
        assert_eq!(server.app, "blender");
        assert_eq!(server.gateway_port, 9765);
        assert!(!server.legacy_gateway_election);
    }

    #[test]
    fn legacy_gateway_election_is_explicit_opt_in() {
        let parsed = Args::try_parse_from([
            "dcc-mcp-server",
            "auto",
            "--app",
            "maya",
            "--legacy-gateway-election",
        ])
        .unwrap();

        let Some(SubCmd::Auto(server)) = parsed.command else {
            panic!("expected auto subcommand");
        };
        assert!(server.legacy_gateway_election);
    }

    #[test]
    fn no_ensure_gateway_is_explicit_opt_out() {
        let parsed =
            Args::try_parse_from(["dcc-mcp-server", "serve", "--no-ensure-gateway"]).unwrap();

        let Some(SubCmd::Serve(serve)) = parsed.command else {
            panic!("expected serve subcommand");
        };
        assert!(serve.server.no_ensure_gateway);
        assert!(!serve.server.legacy_gateway_election);
    }

    #[test]
    fn serve_no_auto_gateway_forces_gateway_port_to_zero() {
        let parsed = Args::try_parse_from([
            "dcc-mcp-server",
            "serve",
            "--no-auto-gateway",
            "--app",
            "maya",
        ])
        .unwrap();

        let Some(SubCmd::Serve(serve)) = parsed.command else {
            panic!("expected serve subcommand");
        };
        let server = serve.into_server_args();
        assert_eq!(server.app, "maya");
        assert_eq!(server.gateway_port, 0);
    }

    #[test]
    fn serve_no_auto_gateway_overrides_gateway_port() {
        let parsed = Args::try_parse_from([
            "dcc-mcp-server",
            "serve",
            "--no-auto-gateway",
            "--gateway-port",
            "1234",
        ])
        .unwrap();

        let Some(SubCmd::Serve(serve)) = parsed.command else {
            panic!("expected serve subcommand");
        };
        let server = serve.into_server_args();
        assert_eq!(server.gateway_port, 0);
    }

    #[test]
    fn root_server_flags_conflict_with_non_server_subcommands() {
        let error =
            Args::try_parse_from(["dcc-mcp-server", "--app", "maya", "gateway"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::ArgumentConflict);
    }

    #[test]
    #[cfg(feature = "gateway-daemon")]
    fn gateway_subcommand_does_not_accept_server_only_flags() {
        let error =
            Args::try_parse_from(["dcc-mcp-server", "gateway", "--app", "maya"]).unwrap_err();

        assert_eq!(error.kind(), ErrorKind::UnknownArgument);
    }
}
