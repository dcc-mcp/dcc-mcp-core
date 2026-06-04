//! Machine-wide standalone gateway daemon and auto-launch helper.

use std::path::PathBuf;
use std::str::FromStr;
use std::time::Duration;

use clap::Args;
use dcc_mcp_gateway::{AdminPersistConfig, GatewayConfig, GatewayRunner, RelaySourceConfig};

#[cfg(feature = "gateway-auto")]
mod guardian;
#[cfg(feature = "gateway-auto")]
mod launcher;

#[cfg(feature = "gateway-auto")]
pub use guardian::{
    GatewayGuardianHandle, GatewayGuardianSettings, GatewayGuardianStatus, spawn_gateway_guardian,
};
#[cfg(feature = "gateway-auto")]
pub use launcher::{EnsureGatewayOptions, ensure_gateway_running};

/// CLI parser for one relay discovery source.
#[derive(Debug, Clone)]
pub struct RelaySourceArg(pub RelaySourceConfig);

impl FromStr for RelaySourceArg {
    type Err = String;

    fn from_str(value: &str) -> Result<Self, Self::Err> {
        let (admin_url, public_base_url) = value.split_once('=').ok_or_else(|| {
            "expected ADMIN_URL=PUBLIC_BASE_URL, for example http://127.0.0.1:9872=http://127.0.0.1:9873"
                .to_string()
        })?;
        let admin_url = admin_url.trim();
        let public_base_url = public_base_url.trim();
        if admin_url.is_empty() || public_base_url.is_empty() {
            return Err("relay source admin and public URLs must be non-empty".into());
        }
        Ok(Self(RelaySourceConfig {
            admin_url: admin_url.to_string(),
            public_base_url: public_base_url.to_string(),
            poll_interval_secs: None,
        }))
    }
}

/// CLI surface for the machine-wide gateway process.
#[derive(Debug, Args, Clone)]
pub struct GatewayArgs {
    /// Gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_HOST", default_value = "127.0.0.1")]
    pub host: String,

    /// Well-known gateway port.
    #[arg(long, env = "DCC_MCP_GATEWAY_PORT", default_value = "9765")]
    pub port: u16,

    /// Human-readable gateway owner label.
    #[arg(long, env = "DCC_MCP_GATEWAY_NAME")]
    pub name: Option<String>,

    /// Remote/LAN gateway host/interface to bind.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_HOST", default_value = "0.0.0.0")]
    pub remote_host: String,

    /// Remote/LAN gateway port. 0 disables the remote listener.
    #[arg(long, env = "DCC_MCP_GATEWAY_REMOTE_PORT", default_value = "59765")]
    pub remote_port: u16,

    /// Directory for the shared FileRegistry.
    #[arg(long, env = "DCC_MCP_REGISTRY_DIR")]
    pub registry_dir: Option<PathBuf>,

    /// Disable the read-only Admin UI.
    #[arg(long, env = "DCC_MCP_NO_ADMIN", default_value = "false")]
    pub no_admin: bool,

    /// URL prefix for the read-only Admin UI.
    #[arg(long, env = "DCC_MCP_ADMIN_PATH", default_value = "/admin")]
    pub admin_path: String,

    /// Seconds without a heartbeat before an instance is considered stale.
    #[arg(long, env = "DCC_MCP_STALE_TIMEOUT", default_value = "30")]
    pub stale_timeout_secs: u64,

    /// Discover LAN-local DCC MCP endpoints via mDNS/DNS-SD.
    #[cfg(feature = "mdns")]
    #[arg(long, env = "DCC_MCP_DISCOVER_MDNS", default_value = "false")]
    pub discover_mdns: bool,

    /// Tunnel relay discovery source, as `ADMIN_URL=PUBLIC_BASE_URL`.
    ///
    /// Repeat the flag or use comma-separated `DCC_MCP_RELAY_SOURCES` values.
    #[arg(
        long = "relay-source",
        env = "DCC_MCP_RELAY_SOURCES",
        value_delimiter = ',',
        value_name = "ADMIN_URL=PUBLIC_BASE_URL"
    )]
    pub relay_sources: Vec<RelaySourceArg>,

    /// Keep the gateway daemon alive even when no backends remain.
    /// Default: false. Use for studio/headless deployments.
    #[arg(long, env = "DCC_MCP_GATEWAY_PERSIST", default_value = "false")]
    pub gateway_persist: bool,

    /// Seconds to wait after the last backend exits before shutting down
    /// the gateway daemon. `0` disables idle timeout (same as `--gateway-persist`).
    /// Default: 30.
    #[arg(long, env = "DCC_MCP_GATEWAY_IDLE_TIMEOUT_SECS", default_value = "30")]
    pub gateway_idle_timeout_secs: u64,

    /// Detach from the terminal and run as a background daemon.
    /// On Unix this performs the classic double-fork; on Windows
    /// the process is spawned detached via the same flags used by
    /// sidecar auto-launch.
    #[arg(long, env = "DCC_MCP_DAEMON", default_value = "false")]
    pub daemon: bool,

    /// Write the daemon process ID to this file. Implicitly enables
    /// --daemon when a path is provided.
    #[arg(long, env = "DCC_MCP_PIDFILE", value_name = "PATH")]
    pub pidfile: Option<PathBuf>,
}

/// Build the [`GatewayConfig`] that the standalone daemon uses.
///
/// Extracted so the regression test can construct the exact same
/// runtime configuration without invoking the blocking `run` loop.
pub fn build_gateway_config(args: &GatewayArgs, gateway_name: &str) -> GatewayConfig {
    let admin_retention = std::env::var("DCC_MCP_GATEWAY_ADMIN_RETENTION_DAYS")
        .ok()
        .and_then(|s| s.parse::<u32>().ok())
        .unwrap_or(30)
        .clamp(1, 3650);
    GatewayConfig {
        host: args.host.clone(),
        gateway_port: args.port,
        remote_host: Some(args.remote_host.clone()),
        remote_gateway_port: args.remote_port,
        stale_timeout_secs: args.stale_timeout_secs,
        server_name: "dcc-mcp-gateway".to_string(),
        gateway_name: Some(gateway_name.to_string()),
        server_version: env!("CARGO_PKG_VERSION").to_string(),
        registry_dir: args.registry_dir.clone(),
        adapter_dcc: Some("gateway".to_string()),
        #[cfg(feature = "mdns")]
        discover_mdns: args.discover_mdns,
        relay_sources: args
            .relay_sources
            .iter()
            .map(|source| source.0.clone())
            .collect(),
        admin_enabled: !args.no_admin,
        admin_path: args.admin_path.clone(),
        admin_persist: AdminPersistConfig {
            sqlite_path: std::env::var_os("DCC_MCP_GATEWAY_ADMIN_DB").map(PathBuf::from),
            sqlite_retention_days: admin_retention,
            ..AdminPersistConfig::default()
        },
        gateway_persist: args.gateway_persist
            || std::env::var("DCC_MCP_GATEWAY_PERSIST")
                .ok()
                .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
                .unwrap_or(false),
        gateway_idle_timeout_secs: args.gateway_idle_timeout_secs,
        ..GatewayConfig::default()
    }
}

fn is_backend_entry(entry: &dcc_mcp_transport::discovery::types::ServiceEntry) -> bool {
    entry.dcc_type != dcc_mcp_transport::discovery::types::GATEWAY_SENTINEL_DCC_TYPE
}

/// Run the standalone gateway until a shutdown signal arrives (or the
/// idle-timeout fires when no backends remain).
pub async fn run(args: GatewayArgs) -> anyhow::Result<()> {
    // ── Daemonize if requested ────────────────────────────────────────────
    let _pidfile = if args.daemon || args.pidfile.is_some() {
        daemonize_gateway(&args)
    } else {
        None
    };

    let gateway_name = args.name.clone().unwrap_or_else(default_gateway_name);
    let cfg = build_gateway_config(&args, &gateway_name);
    let runner =
        GatewayRunner::new(cfg).map_err(|err| anyhow::anyhow!("creating GatewayRunner: {err}"))?;
    let mut outcome = runner
        .run_election()
        .await
        .map_err(|err| anyhow::anyhow!("running gateway election: {err}"))?;

    if !outcome.is_gateway {
        tracing::info!(
            host = %args.host,
            port = args.port,
            "standalone gateway found an existing owner; exiting"
        );
        return Ok(());
    }

    let gateway_persist = args.gateway_persist
        || std::env::var("DCC_MCP_GATEWAY_PERSIST")
            .ok()
            .map(|v| v == "1" || v.eq_ignore_ascii_case("true"))
            .unwrap_or(false);
    let gateway_idle_timeout_secs = args.gateway_idle_timeout_secs;

    tracing::info!(
        gateway_name = %gateway_name,
        host = %args.host,
        port = args.port,
        gateway_persist,
        gateway_idle_timeout_secs,
        "standalone gateway running"
    );

    // ── Idle-timeout watch (PIP-487) ───────────────────────────────────────
    //
    // Polls the FileRegistry for live backends. When no backend remains and
    // persistence is off, starts a countdown. Backends that reconnect during
    // the grace period cancel the timer; expiry triggers an orderly shutdown.
    let idle_shutdown = if !gateway_persist && gateway_idle_timeout_secs > 0 {
        let registry = runner.registry.clone();
        let (idle_tx, idle_rx) = tokio::sync::watch::channel(false);
        tokio::spawn(async move {
            let poll = Duration::from_secs(5);
            let grace = Duration::from_secs(gateway_idle_timeout_secs);
            let mut idle_since: Option<std::time::Instant> = None;

            loop {
                tokio::time::sleep(poll).await;
                let live_count = {
                    match registry.try_read() {
                        Ok(reg) => reg.list_all().into_iter().filter(is_backend_entry).count(),
                        Err(_) => continue,
                    }
                };

                if live_count > 0 {
                    if idle_since.take().is_some() {
                        tracing::info!(
                            live_backends = live_count,
                            "gateway idle countdown cancelled — backends reconnected"
                        );
                    }
                    continue;
                }

                let since = *idle_since.get_or_insert_with(std::time::Instant::now);
                let elapsed = since.elapsed();
                tracing::debug!(
                    elapsed_secs = elapsed.as_secs(),
                    grace_period_secs = grace.as_secs(),
                    "gateway idle: no live backends"
                );

                if elapsed >= grace {
                    tracing::warn!(
                        grace_period_secs = grace.as_secs(),
                        "gateway idle timeout reached — shutting down"
                    );
                    let _ = idle_tx.send(true);
                    return;
                }
            }
        });
        Some(idle_rx)
    } else {
        None
    };

    // ── Wait for shutdown trigger ──────────────────────────────────────────
    let shutdown_reason = if let Some(mut idle_rx) = idle_shutdown {
        tokio::select! {
            sig = crate::select_shutdown_signal() => {
                sig.unwrap_or("signal")
            }
            _ = idle_rx.changed() => {
                "idle_timeout"
            }
        }
    } else {
        crate::select_shutdown_signal().await?
    };
    tracing::info!(shutdown_reason, "standalone gateway shutting down");

    if let Some(abort) = outcome.gateway_abort.take() {
        abort.abort();
    }
    if let Some(key) = outcome.sentinel_key.take() {
        let reg = runner.registry.read().await;
        let _ = reg.deregister(&key);
    }

    // Drop the pidfile guard so the file is cleaned up.
    drop(_pidfile);
    Ok(())
}

/// Daemonize the gateway process and return a pidfile guard.
#[cfg(unix)]
fn daemonize_gateway(args: &GatewayArgs) -> Option<PidfileGuard> {
    // Double-fork: parent exits, child becomes session leader.
    let pid = unsafe { libc::fork() };
    if pid > 0 {
        std::process::exit(0);
    }
    unsafe { libc::setsid() };
    let pid = unsafe { libc::fork() };
    if pid > 0 {
        std::process::exit(0);
    }

    // Redirect stdio to /dev/null.
    if let Ok(devnull) = std::fs::OpenOptions::new()
        .read(true)
        .write(true)
        .open("/dev/null")
    {
        use std::os::unix::io::AsRawFd;
        let fd = devnull.as_raw_fd();
        unsafe {
            libc::dup2(fd, 0);
            libc::dup2(fd, 1);
            libc::dup2(fd, 2);
        }
    }

    let guard = write_gateway_pidfile(&args.pidfile);
    if args.daemon && args.pidfile.is_none() {
        tracing::info!(pid = std::process::id(), "gateway daemonized");
    }
    guard
}

#[cfg(not(unix))]
fn daemonize_gateway(_args: &GatewayArgs) -> Option<PidfileGuard> {
    // Windows: the process is already detached when launched via
    // spawn_detached_gateway(). Just write the pidfile.
    let guard = write_gateway_pidfile(&_args.pidfile);
    if _args.daemon && _args.pidfile.is_none() {
        tracing::info!(pid = std::process::id(), "gateway running detached");
    }
    guard
}

/// ```ignore
/// A sentinel that removes the pidfile on Drop.
/// ```
struct PidfileGuard {
    path: PathBuf,
}

impl Drop for PidfileGuard {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn write_gateway_pidfile(pidfile: &Option<PathBuf>) -> Option<PidfileGuard> {
    let path = pidfile.as_ref()?;
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(path, format!("{}\n", std::process::id())).ok()?;
    Some(PidfileGuard { path: path.clone() })
}

fn default_gateway_name() -> String {
    let host = std::env::var("COMPUTERNAME")
        .or_else(|_| std::env::var("HOSTNAME"))
        .unwrap_or_else(|_| "local".to_string());
    format!("gateway-{host}-pid{}", std::process::id())
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::{GATEWAY_SENTINEL_DCC_TYPE, ServiceEntry};

    #[test]
    fn relay_source_arg_maps_into_gateway_config() {
        let source: RelaySourceArg = "http://127.0.0.1:9872=http://127.0.0.1:9873"
            .parse()
            .expect("relay source arg should parse");

        assert_eq!(source.0.admin_url, "http://127.0.0.1:9872");
        assert_eq!(source.0.public_base_url, "http://127.0.0.1:9873");
        assert!(source.0.poll_interval_secs.is_none());
        assert!(
            "http://127.0.0.1:9872=".parse::<RelaySourceArg>().is_err(),
            "empty public relay URL must be rejected"
        );

        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: Some("relay-source-test".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: None,
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: vec![source],
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
            daemon: false,
            pidfile: None,
        };

        let cfg = build_gateway_config(&args, "relay-source-test");
        assert_eq!(cfg.relay_sources.len(), 1);
        assert_eq!(cfg.relay_sources[0].admin_url, "http://127.0.0.1:9872");
        assert_eq!(
            cfg.relay_sources[0].public_base_url,
            "http://127.0.0.1:9873"
        );
    }

    fn ephemeral_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
    }

    #[test]
    fn idle_lifecycle_counts_backends_on_same_host_as_gateway() {
        let maya = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", 18813);
        let gateway = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);

        assert!(is_backend_entry(&maya));
        assert!(is_backend_entry(&photoshop));
        assert!(!is_backend_entry(&gateway));
    }

    /// Issue #1358 — the standalone gateway daemon must serve gateway-
    /// native endpoints **without any DCC backend being registered**. The
    /// daemon's `adapter_dcc = Some("gateway")` marker is what distinguishes
    /// it from a per-DCC server that happens to win the election.
    #[tokio::test]
    async fn standalone_daemon_serves_health_without_any_backend() {
        let dir = tempfile::tempdir().unwrap();
        let gw_port = ephemeral_port();
        let args = GatewayArgs {
            host: "127.0.0.1".to_string(),
            port: gw_port,
            name: Some("standalone-daemon-test".to_string()),
            remote_host: "127.0.0.1".to_string(),
            remote_port: 0,
            registry_dir: Some(dir.path().to_path_buf()),
            no_admin: true,
            admin_path: "/admin".to_string(),
            stale_timeout_secs: 30,
            #[cfg(feature = "mdns")]
            discover_mdns: false,
            relay_sources: Vec::new(),
            gateway_persist: false,
            gateway_idle_timeout_secs: 30,
            daemon: false,
            pidfile: None,
        };
        let cfg = build_gateway_config(&args, args.name.as_deref().unwrap());
        assert_eq!(
            cfg.adapter_dcc.as_deref(),
            Some("gateway"),
            "daemon-mode config must stamp the standalone marker"
        );

        let runner = GatewayRunner::new(cfg).expect("creating GatewayRunner");
        let mut outcome = runner
            .run_election()
            .await
            .expect("standalone daemon must win election on a free port");
        assert!(
            outcome.is_gateway,
            "standalone daemon must elect itself as the gateway"
        );

        let client = reqwest::Client::builder()
            .timeout(std::time::Duration::from_secs(2))
            .build()
            .unwrap();
        let health = client
            .get(format!("http://127.0.0.1:{gw_port}/health"))
            .send()
            .await
            .expect("daemon must answer /health without a DCC backend");
        assert!(health.status().is_success(), "/health expected 200 OK");

        if let Some(abort) = outcome.gateway_abort.take() {
            abort.abort();
        }
        if let Some(key) = outcome.sentinel_key.take() {
            let reg = runner.registry.read().await;
            let _ = reg.deregister(&key);
        }
    }
}
