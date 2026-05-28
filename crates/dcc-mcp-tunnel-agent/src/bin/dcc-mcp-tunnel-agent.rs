//! Operator-facing CLI for the DCC-MCP tunnel agent (issue #504).
//!
//! Wraps [`run_with_reconnect`] in a long-lived process with SIGINT /
//! SIGTERM (or Ctrl+C on Windows) clean shutdown. The agent keeps
//! reconnecting to the relay on transient failures; a non-retryable
//! `Rejected` (bad JWT / `allowed_dcc` mismatch) exits the process with
//! a non-zero code so supervisors do not restart-loop on a
//! misconfiguration.
//!
//! Configuration is taken from CLI flags with matching
//! `DCC_MCP_TUNNEL_AGENT_*` environment variables as fallbacks. The
//! bearer JWT is expected via `--token-file` so it never reaches
//! process argv.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::{Context, Result};
use clap::{Parser, ValueEnum};
use dcc_mcp_tunnel_agent::{AgentConfig, ReconnectExit, ReconnectPolicy, run_with_reconnect};
use tokio::sync::watch;
use tracing_subscriber::EnvFilter;

#[derive(Copy, Clone, Debug, PartialEq, Eq, ValueEnum)]
enum ReconnectMode {
    /// Constant delay between attempts; see `--reconnect-constant-secs`.
    Constant,
    /// Exponential back-off; see `--reconnect-initial-secs` / `--reconnect-max-secs`.
    Exponential,
}

/// DCC-MCP tunnel agent — bridges a local MCP HTTP server to a public
/// `dcc-mcp-tunnel-relay`.
#[derive(Debug, Parser)]
#[command(name = "dcc-mcp-tunnel-agent", about, version)]
struct Args {
    /// Relay WebSocket URL (`wss://relay.example.com`). Required.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_RELAY_URL")]
    relay_url: String,

    /// Path to a file containing the bearer JWT minted by
    /// `dcc_mcp_tunnel_protocol::auth::issue`. Required.
    ///
    /// Passed by file so the token never appears in `ps` output.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_TOKEN_FILE", value_name = "PATH")]
    token_file: PathBuf,

    /// DCC tag this agent identifies with (must be in the JWT's
    /// `allowed_dcc` list). E.g. `maya`, `blender`, `houdini`.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_DCC")]
    dcc: String,

    /// Local MCP HTTP server address (`host:port`) to bridge to. On
    /// every remote `OpenSession`, the agent opens a fresh TCP
    /// connection here.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_LOCAL_TARGET")]
    local_target: String,

    /// Heartbeat cadence in seconds. Keep comfortably under the
    /// relay's `--stale-timeout-secs` so one missed ping does not evict
    /// the tunnel.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_HEARTBEAT_SECS",
        default_value = "10"
    )]
    heartbeat_secs: u64,

    /// Reconnect policy when the relay leg drops.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_RECONNECT_POLICY",
        value_enum,
        default_value_t = ReconnectMode::Exponential,
    )]
    reconnect_policy: ReconnectMode,

    /// `Exponential`: initial delay before the first retry (seconds).
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_RECONNECT_INITIAL_SECS",
        default_value = "2"
    )]
    reconnect_initial_secs: u64,

    /// `Exponential`: hard cap on the delay between retries (seconds).
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_RECONNECT_MAX_SECS",
        default_value = "60"
    )]
    reconnect_max_secs: u64,

    /// `Constant`: flat delay between retry attempts (seconds).
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_RECONNECT_CONSTANT_SECS",
        default_value = "5"
    )]
    reconnect_constant_secs: u64,

    /// Optional comma-separated capability tags forwarded to remote
    /// clients via the relay's `/tunnels` listing.
    #[arg(
        long,
        env = "DCC_MCP_TUNNEL_AGENT_CAPABILITIES",
        value_delimiter = ',',
        num_args = 0..,
    )]
    capabilities: Vec<String>,

    /// Stable DCC instance UUID to expose through relay-aware gateways.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_INSTANCE_ID")]
    instance_id: Option<String>,

    /// Fingerprint of the backend capability set.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_CAPABILITIES_FINGERPRINT")]
    capabilities_fingerprint: Option<String>,

    /// Adapter package version to expose through `/tunnels`.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_ADAPTER_VERSION")]
    adapter_version: Option<String>,

    /// Currently active scene or document.
    #[arg(long, env = "DCC_MCP_TUNNEL_AGENT_SCENE")]
    scene: Option<String>,
}

fn init_tracing() {
    let filter = EnvFilter::try_from_env("MCP_LOG_LEVEL")
        .or_else(|_| EnvFilter::try_from_env("RUST_LOG"))
        .unwrap_or_else(|_| EnvFilter::new("info"));
    tracing_subscriber::fmt().with_env_filter(filter).init();
}

fn load_token(path: &std::path::Path) -> Result<String> {
    let raw = std::fs::read_to_string(path)
        .with_context(|| format!("reading bearer token from {}", path.display()))?;
    let token = raw.trim().to_string();
    anyhow::ensure!(
        !token.is_empty(),
        "token file {} is empty after trimming whitespace",
        path.display()
    );
    Ok(token)
}

#[tokio::main]
async fn main() -> Result<()> {
    init_tracing();
    let args = Args::parse();

    let token = load_token(&args.token_file)?;

    let reconnect = match args.reconnect_policy {
        ReconnectMode::Constant => ReconnectPolicy::Constant {
            delay: Duration::from_secs(args.reconnect_constant_secs),
        },
        ReconnectMode::Exponential => ReconnectPolicy::Exponential {
            initial: Duration::from_secs(args.reconnect_initial_secs),
            max: Duration::from_secs(args.reconnect_max_secs),
        },
    };

    let mut cfg = AgentConfig::new(args.relay_url, token, args.dcc, args.local_target);
    cfg.heartbeat_interval = Duration::from_secs(args.heartbeat_secs);
    cfg.reconnect = reconnect;
    cfg.capabilities = args.capabilities;
    cfg.instance_id = args.instance_id;
    cfg.capabilities_fingerprint = args.capabilities_fingerprint;
    cfg.adapter_version = args.adapter_version;
    cfg.scene = args.scene;

    tracing::info!(
        relay = %cfg.relay_url,
        dcc = %cfg.dcc,
        local = %cfg.local_target,
        policy = ?cfg.reconnect,
        "starting dcc-mcp-tunnel-agent"
    );

    let (shutdown_tx, shutdown_rx) = watch::channel(false);

    // Spawn the agent loop; wait on it concurrently with the signal
    // watcher so whichever completes first drives the exit path.
    let agent_handle = tokio::spawn(run_with_reconnect(cfg, shutdown_rx));

    let signal_task = tokio::spawn(wait_for_shutdown_signal(shutdown_tx));

    let exit = agent_handle.await.context("agent task panicked")?;
    // Make sure the signal watcher is woken so we don't leak the task
    // on a Fatal exit.
    signal_task.abort();

    match exit {
        ReconnectExit::Shutdown => {
            tracing::info!("dcc-mcp-tunnel-agent stopped cleanly");
            Ok(())
        }
        ReconnectExit::Fatal(err) => {
            tracing::error!(error = %err, "dcc-mcp-tunnel-agent hit a non-retryable error");
            Err(anyhow::anyhow!(err))
        }
    }
}

async fn wait_for_shutdown_signal(shutdown_tx: watch::Sender<bool>) {
    wait_for_shutdown().await;
    let _ = shutdown_tx.send(true);
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
