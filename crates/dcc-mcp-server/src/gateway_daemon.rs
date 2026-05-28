//! Machine-wide standalone gateway daemon and auto-launch helper.

#[cfg(feature = "gateway-auto")]
use std::ffi::OsString;
#[cfg(feature = "gateway-auto")]
use std::fs::{File, OpenOptions};
use std::path::PathBuf;
#[cfg(feature = "gateway-auto")]
use std::process::{Command, Stdio};
#[cfg(feature = "gateway-auto")]
use std::time::{Duration, Instant};

#[cfg(feature = "gateway-auto")]
use anyhow::Context as _;
use clap::Args;
use dcc_mcp_gateway::{AdminPersistConfig, GatewayConfig, GatewayRunner};

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
}

/// Helpers for auto-launching the standalone gateway from inside another
/// process (the per-DCC sidecar / embedded server). Only needed when the
/// `gateway-auto` feature is on; pure daemon builds skip them entirely.
#[cfg(feature = "gateway-auto")]
#[derive(Debug, Clone)]
pub struct EnsureGatewayOptions {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub registry_dir: PathBuf,
    pub remote_host: String,
    pub remote_port: u16,
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
        admin_enabled: !args.no_admin,
        admin_path: args.admin_path.clone(),
        admin_persist: AdminPersistConfig {
            sqlite_path: std::env::var_os("DCC_MCP_GATEWAY_ADMIN_DB").map(PathBuf::from),
            sqlite_retention_days: admin_retention,
            ..AdminPersistConfig::default()
        },
        ..GatewayConfig::default()
    }
}

/// Run the standalone gateway until a shutdown signal arrives.
pub async fn run(args: GatewayArgs) -> anyhow::Result<()> {
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

    tracing::info!(
        gateway_name = %gateway_name,
        host = %args.host,
        port = args.port,
        "standalone gateway running"
    );

    let shutdown_reason = crate::select_shutdown_signal().await?;
    tracing::info!(shutdown_reason, "standalone gateway shutting down");

    if let Some(abort) = outcome.gateway_abort.take() {
        abort.abort();
    }
    if let Some(key) = outcome.sentinel_key.take() {
        let reg = runner.registry.read().await;
        let _ = reg.deregister(&key);
    }
    Ok(())
}

/// Ensure the machine-wide gateway is reachable, launching it once if needed.
#[cfg(feature = "gateway-auto")]
pub async fn ensure_gateway_running(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    if opts.port == 0 || gateway_health_ok(&opts.host, opts.port).await {
        return Ok(());
    }

    std::fs::create_dir_all(&opts.registry_dir)
        .with_context(|| format!("creating registry dir {}", opts.registry_dir.display()))?;
    let lock_path = opts.registry_dir.join("gateway-launch.lock");
    match acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            if gateway_health_ok(&opts.host, opts.port).await {
                return Ok(());
            }
            spawn_detached_gateway(opts)?;
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            tracing::info!(
                path = %lock_path.display(),
                "another process is launching the gateway"
            );
        }
        Err(err) => return Err(err).with_context(|| format!("creating {}", lock_path.display())),
    }

    wait_gateway_ready(&opts.host, opts.port, Duration::from_secs(10)).await
}

#[cfg(feature = "gateway-auto")]
async fn wait_gateway_ready(host: &str, port: u16, timeout: Duration) -> anyhow::Result<()> {
    let deadline = Instant::now() + timeout;
    while Instant::now() < deadline {
        if gateway_health_ok(host, port).await {
            return Ok(());
        }
        tokio::time::sleep(Duration::from_millis(150)).await;
    }
    anyhow::bail!(
        "gateway did not become healthy at http://{host}:{port}/health within {timeout:?}"
    )
}

#[cfg(feature = "gateway-auto")]
async fn gateway_health_ok(host: &str, port: u16) -> bool {
    let url = format!("http://{host}:{port}/health");
    let client = match reqwest::Client::builder()
        .timeout(Duration::from_millis(600))
        .build()
    {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

#[cfg(feature = "gateway-auto")]
struct LaunchLock {
    _file: File,
    path: PathBuf,
}

#[cfg(feature = "gateway-auto")]
impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

#[cfg(feature = "gateway-auto")]
fn acquire_launch_lock(path: &std::path::Path) -> std::io::Result<LaunchLock> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|file| LaunchLock {
            _file: file,
            path: path.to_path_buf(),
        })
}

#[cfg(feature = "gateway-auto")]
fn spawn_detached_gateway(opts: &EnsureGatewayOptions) -> anyhow::Result<()> {
    let exe = std::env::current_exe().context("resolving current executable")?;
    let mut cmd = Command::new(exe);
    cmd.args(gateway_command_args(opts))
        .env("DCC_MCP_REGISTRY_DIR", &opts.registry_dir)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null());

    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        const DETACHED_PROCESS: u32 = 0x0000_0008;
        cmd.creation_flags(CREATE_NO_WINDOW | DETACHED_PROCESS);
    }

    cmd.spawn().context("spawning standalone gateway")?;
    tracing::info!(port = opts.port, "spawned standalone gateway process");
    Ok(())
}

#[cfg(feature = "gateway-auto")]
fn gateway_command_args(opts: &EnsureGatewayOptions) -> Vec<OsString> {
    let mut args = vec![
        OsString::from("gateway"),
        OsString::from("--host"),
        OsString::from(&opts.host),
        OsString::from("--port"),
        OsString::from(opts.port.to_string()),
        OsString::from("--remote-host"),
        OsString::from(&opts.remote_host),
        OsString::from("--remote-port"),
        OsString::from(opts.remote_port.to_string()),
    ];
    if let Some(name) = opts.name.as_ref().filter(|name| !name.trim().is_empty()) {
        args.push(OsString::from("--name"));
        args.push(OsString::from(name));
    }
    args
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

    #[cfg(feature = "gateway-auto")]
    #[test]
    fn auto_launch_gateway_args_do_not_include_registry_dir_flag() {
        let opts = EnsureGatewayOptions {
            host: "127.0.0.1".to_string(),
            port: 9765,
            name: Some("gateway-for-test".to_string()),
            registry_dir: PathBuf::from(r"C:\tmp\dcc-mcp-registry"),
            remote_host: "0.0.0.0".to_string(),
            remote_port: 59765,
        };

        let args: Vec<String> = gateway_command_args(&opts)
            .into_iter()
            .map(|arg| arg.to_string_lossy().to_string())
            .collect();

        assert!(
            !args.iter().any(|arg| arg == "--registry-dir"),
            "auto-launched gateway should inherit DCC_MCP_REGISTRY_DIR instead of exposing --registry-dir in the command line"
        );
        assert!(args.iter().any(|arg| arg == "gateway"));
        assert!(args.iter().any(|arg| arg == "--name"));
    }

    fn ephemeral_port() -> u16 {
        let listener = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
        let port = listener.local_addr().unwrap().port();
        drop(listener);
        port
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
