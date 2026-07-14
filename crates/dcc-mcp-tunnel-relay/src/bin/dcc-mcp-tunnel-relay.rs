//! Operator-facing CLI for the DCC-MCP tunnel relay (issue #504).
//!
//! Boots a [`RelayServer`] bound to the supplied addresses and blocks
//! until SIGINT / SIGTERM (or, on Windows, Ctrl+C) is received.
//!
//! Configuration is taken from CLI flags with matching `DCC_MCP_TUNNEL_RELAY_*`
//! environment variables as fallbacks. The JWT secret is expected to
//! arrive via `--jwt-secret-file` so the bytes never hit the process
//! argument list or `ps` output.
//!
//! Example:
//!
//! ```bash
//! dcc-mcp-tunnel-relay \
//!     --jwt-secret-file /etc/dcc-mcp/tunnel-secret \
//!     --public-host relay.example.com \
//!     --base-url wss://relay.example.com \
//!     --agent-bind 0.0.0.0:9870 \
//!     --frontend-bind 0.0.0.0:9871 \
//!     --admin-bind 127.0.0.1:9877
//! ```

use std::net::SocketAddr;
use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::Parser;
use dcc_mcp_tunnel_relay::{OptionalBinds, RelayConfig, RelayServer};
use tracing_subscriber::EnvFilter;

/// DCC-MCP tunnel relay — accepts WebSocket registrations from local
/// tunnel agents and forwards remote MCP sessions to them.
#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-tunnel-relay", about, version)]
struct Args {
    /// Path to a file containing the HS256 JWT secret. Required.
    ///
    /// The secret must be at least 32 bytes of entropy in production
    /// (`openssl rand -base64 48`). Passed by file so the bytes never
    /// appear in `ps` output or shell history.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_JWT_SECRET_FILE",
        value_name = "PATH"
    )]
    jwt_secret_file: PathBuf,

    /// Public hostname the relay advertises in minted tunnel URLs
    /// (ends up in the JWT `iss` claim).
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_PUBLIC_HOST",
        default_value = "localhost"
    )]
    public_host: String,

    /// WebSocket base URL prepended to per-tunnel paths in
    /// `RegisterAck.public_url`.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_BASE_URL",
        default_value = "ws://localhost:9870"
    )]
    base_url: String,

    /// TCP bind for the agent control plane (where local tunnel agents
    /// register).
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_AGENT_BIND",
        default_value = "0.0.0.0:9870"
    )]
    agent_bind: SocketAddr,

    /// TCP bind for the raw-socket remote-client frontend.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_FRONTEND_BIND",
        default_value = "0.0.0.0:9871"
    )]
    frontend_bind: SocketAddr,

    /// Optional bind for the WebSocket frontend (`/tunnel/<id>` upgrade).
    /// Omit to disable.
    #[arg(long, env = "DCC_MCP_TUNNEL_RELAY_WS_FRONTEND_BIND")]
    ws_frontend_bind: Option<SocketAddr>,

    /// Optional bind for the read-only admin endpoint
    /// (`GET /tunnels`, `GET /healthz`). Omit to disable.
    #[arg(long, env = "DCC_MCP_TUNNEL_RELAY_ADMIN_BIND")]
    admin_bind: Option<SocketAddr>,

    /// Seconds without a heartbeat before a tunnel is evicted from the
    /// registry.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_RELAY_STALE_TIMEOUT_SECS",
        default_value = "30"
    )]
    stale_timeout_secs: u64,

    /// Hard cap on simultaneously-registered tunnels. `0` disables the cap.
    #[arg(long, env = "DCC_MCP_TUNNEL_RELAY_MAX_TUNNELS", default_value = "0")]
    max_tunnels: usize,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("DCC_MCP_LOG_LEVEL")
        .or_else(|_| EnvFilter::try_from_env("MCP_LOG_LEVEL"))
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn load_jwt_secret(path: &std::path::Path) -> Result<Vec<u8>> {
    let raw = std::fs::read(path)
        .with_context(|| format!("reading JWT secret from {}", path.display()))?;
    // Allow trailing whitespace / newlines (common for shell-piped
    // secrets). Operators should still keep the file tight via chmod.
    let trimmed_end = raw.iter().rposition(|b| !b.is_ascii_whitespace());
    let secret = match trimmed_end {
        Some(end) => raw[..=end].to_vec(),
        None => Vec::new(),
    };
    anyhow::ensure!(
        !secret.is_empty(),
        "JWT secret file {} is empty after trimming whitespace",
        path.display()
    );
    if secret.len() < 32 {
        tracing::warn!(
            path = %path.display(),
            bytes = secret.len(),
            "JWT secret is shorter than 32 bytes; generate a longer one for production (openssl rand -base64 48)"
        );
    }
    Ok(secret)
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();

    let jwt_secret = load_jwt_secret(&args.jwt_secret_file)?;
    let cfg = RelayConfig {
        jwt_secret,
        public_host: args.public_host,
        base_url: args.base_url,
        stale_timeout: Duration::from_secs(args.stale_timeout_secs),
        max_tunnels: args.max_tunnels,
    };

    let optional = OptionalBinds {
        ws_frontend: args.ws_frontend_bind,
        admin: args.admin_bind,
    };

    tracing::info!(
        agent_bind = %args.agent_bind,
        frontend_bind = %args.frontend_bind,
        ws_frontend_bind = ?args.ws_frontend_bind,
        admin_bind = ?args.admin_bind,
        "starting dcc-mcp-tunnel-relay"
    );

    let server = RelayServer::start_with(cfg, args.agent_bind, args.frontend_bind, optional)
        .await
        .context("binding relay listeners")?;

    tracing::info!(
        agent_addr = %server.agent_addr,
        frontend_addr = %server.frontend_addr,
        ws_frontend_addr = ?server.ws_frontend_addr,
        admin_addr = ?server.admin_addr,
        "dcc-mcp-tunnel-relay listening"
    );

    wait_for_shutdown().await;
    tracing::info!("shutdown signal received; draining");
    // `RelayServer::shutdown` drops the accept loops; live sessions
    // wind down on their own as their sockets close.
    drop(server);
    Ok(())
}

#[cfg(unix)]
async fn wait_for_shutdown() {
    use tokio::signal::unix::{SignalKind, signal};
    let mut sigint = signal(SignalKind::interrupt()).expect("install SIGINT handler");
    let mut sigterm = signal(SignalKind::terminate()).expect("install SIGTERM handler");
    tokio::select! {
        _ = sigint.recv() => tracing::info!("got SIGINT"),
        _ = sigterm.recv() => tracing::info!("got SIGTERM"),
    }
}

#[cfg(not(unix))]
async fn wait_for_shutdown() {
    let _ = tokio::signal::ctrl_c().await;
    tracing::info!("got Ctrl+C");
}
