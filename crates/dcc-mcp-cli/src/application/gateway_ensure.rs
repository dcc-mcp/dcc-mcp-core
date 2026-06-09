//! Gateway health check and auto-launch helpers.
//!
//! Ported from `dcc-mcp-sidecar`'s `gateway_daemon::launcher` and simplified:
//! - No version takeover (CLI is not a DCC adapter).
//! - No FileRegistry dependency.
//! - No adapter_version / adapter_dcc fields.
//!
//! Shared primitives (health check, launch lock, spawn, pidfile, process
//! utilities) live in `dcc-mcp-gateway-ensure`.

use std::path::PathBuf;
use std::time::Duration;

use anyhow::Context;
use dcc_mcp_gateway_ensure as ensure;
use serde::Serialize;

use super::gateway_discovery;

/// Outcome of an `ensure_gateway_running` call.
#[derive(Debug, Clone, Serialize)]
pub struct EnsureResult {
    pub host: String,
    pub port: u16,
    pub already_running: bool,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub pid: Option<u32>,
}

/// Parameters for `ensure_gateway_running`.
#[derive(Debug, Clone)]
pub struct EnsureGatewayArgs {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub registry_dir: PathBuf,
    pub remote_host: String,
    pub remote_port: u16,
    pub gateway_idle_timeout_secs: u64,
    pub gateway_bin: Option<PathBuf>,
    pub wait_timeout_secs: u64,
    /// Optional path for the PID file written after a successful start.
    pub pidfile: Option<PathBuf>,
}

/// Ensure the gateway is reachable at `host:port`, launching it once if needed.
pub async fn ensure_gateway_running(args: &EnsureGatewayArgs) -> anyhow::Result<EnsureResult> {
    if args.port == 0 {
        anyhow::bail!("gateway port must be non-zero");
    }

    if ensure::gateway_health_ok(&args.host, args.port).await {
        return Ok(EnsureResult {
            host: args.host.clone(),
            port: args.port,
            already_running: true,
            pid: ensure::read_pid_from_pidfile(args.pidfile.as_deref()),
        });
    }

    std::fs::create_dir_all(&args.registry_dir)
        .with_context(|| format!("creating registry dir {}", args.registry_dir.display()))?;
    let lock_path = args.registry_dir.join("gateway-launch.lock");
    match ensure::acquire_launch_lock(&lock_path) {
        Ok(_lock) => {
            // Double-check after acquiring the lock (race protection).
            if ensure::gateway_health_ok(&args.host, args.port).await {
                return Ok(EnsureResult {
                    host: args.host.clone(),
                    port: args.port,
                    already_running: true,
                    pid: ensure::read_pid_from_pidfile(args.pidfile.as_deref()),
                });
            }

            let exe = resolve_gateway_bin(args).await?;
            let cmd_args = ensure::gateway_command_args(
                &args.host,
                args.port,
                args.name.as_deref(),
                &args.remote_host,
                args.remote_port,
                args.gateway_idle_timeout_secs,
            );
            let pid =
                ensure::spawn_detached_gateway(&exe, &cmd_args, &args.registry_dir)?;

            ensure::wait_gateway_ready(
                &args.host,
                args.port,
                Duration::from_secs(ensure::resolve_ensure_timeout_secs(
                    args.wait_timeout_secs,
                )),
            )
            .await?;

            // Release lock after gateway is confirmed ready.
            drop(_lock);

            // Write PID file so stop/status commands can find the process.
            if let Some(ref pidfile) = args.pidfile {
                ensure::write_pidfile(pidfile, pid)?;
            }

            Ok(EnsureResult {
                host: args.host.clone(),
                port: args.port,
                already_running: false,
                pid: Some(pid),
            })
        }
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            // Another process holds the launch lock — wait for its winner to
            // finish and check whether the gateway becomes healthy (mirrors
            // Python `_wait_gateway_ready` on lock-loser path).
            let timeout = Duration::from_secs(ensure::resolve_ensure_timeout_secs(
                args.wait_timeout_secs,
            ));
            ensure::wait_gateway_ready(&args.host, args.port, timeout).await?;
            Ok(EnsureResult {
                host: args.host.clone(),
                port: args.port,
                already_running: true,
                pid: ensure::read_pid_from_pidfile(args.pidfile.as_deref()),
            })
        }
        Err(err) => {
            Err(err).with_context(|| format!("creating launch lock {}", lock_path.display()))?
        }
    }
}

// ── Binary resolution (CLI-specific: uses gateway_discovery) ────────────────

async fn resolve_gateway_bin(args: &EnsureGatewayArgs) -> anyhow::Result<PathBuf> {
    gateway_discovery::resolve_gateway_bin(args.gateway_bin.as_ref()).await
}

// ── Re-exports for convenience ──────────────────────────────────────────────

pub use ensure::{
    default_registry_dir, gateway_health_ok, is_process_alive, read_pid_from_pidfile,
    remove_pidfile, stop_process,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_gateway_command_args_minimal() {
        let argv: Vec<String> = ensure::gateway_command_args(
            "127.0.0.1", 9765, None, "0.0.0.0", 59765, 30,
        )
        .into_iter()
        .map(|s| s.to_string_lossy().to_string())
        .collect();
        assert!(argv[0] == "gateway");
        assert!(argv.contains(&"--port".to_string()));
        assert!(argv.contains(&"9765".to_string()));
    }

    #[test]
    fn test_default_registry_dir_is_not_empty() {
        let dir = default_registry_dir();
        assert!(!dir.as_os_str().is_empty());
        assert!(dir.is_absolute());
    }
}
