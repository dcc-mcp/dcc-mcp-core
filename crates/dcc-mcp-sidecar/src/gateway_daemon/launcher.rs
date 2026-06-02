use std::ffi::OsString;
use std::fs::{File, OpenOptions};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};
use std::time::{Duration, Instant};

use anyhow::Context as _;

const ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS: &str = "DCC_MCP_GATEWAY_LAUNCH_LOCK_STALE_SECS";
const DEFAULT_GATEWAY_LAUNCH_LOCK_STALE_SECS: u64 = 30;

/// Helpers for auto-launching the standalone gateway from inside another
/// process (the per-DCC sidecar / embedded server).
#[derive(Debug, Clone)]
pub struct EnsureGatewayOptions {
    pub host: String,
    pub port: u16,
    pub name: Option<String>,
    pub registry_dir: PathBuf,
    pub remote_host: String,
    pub remote_port: u16,
}

/// Ensure the machine-wide gateway is reachable, launching it once if needed.
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

pub(super) async fn gateway_health_ok(host: &str, port: u16) -> bool {
    gateway_health_ok_with_timeout(host, port, Duration::from_millis(600)).await
}

pub(super) async fn gateway_health_ok_with_timeout(
    host: &str,
    port: u16,
    timeout: Duration,
) -> bool {
    let url = format!("http://{host}:{port}/health");
    let client = match reqwest::Client::builder().timeout(timeout).build() {
        Ok(client) => client,
        Err(_) => return false,
    };
    client
        .get(url)
        .send()
        .await
        .is_ok_and(|resp| resp.status().is_success())
}

struct LaunchLock {
    _file: File,
    path: PathBuf,
}

impl Drop for LaunchLock {
    fn drop(&mut self) {
        let _ = std::fs::remove_file(&self.path);
    }
}

fn acquire_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    acquire_launch_lock_with_stale(path, gateway_launch_lock_stale_after())
}

fn acquire_launch_lock_with_stale(
    path: &Path,
    stale_after: Duration,
) -> std::io::Result<LaunchLock> {
    match create_launch_lock(path) {
        Ok(lock) => Ok(lock),
        Err(err) if err.kind() == std::io::ErrorKind::AlreadyExists => {
            if remove_stale_launch_lock(path, stale_after)? {
                create_launch_lock(path)
            } else {
                Err(err)
            }
        }
        Err(err) => Err(err),
    }
}

fn create_launch_lock(path: &Path) -> std::io::Result<LaunchLock> {
    OpenOptions::new()
        .write(true)
        .create_new(true)
        .open(path)
        .map(|file| LaunchLock {
            _file: file,
            path: path.to_path_buf(),
        })
}

fn gateway_launch_lock_stale_after() -> Duration {
    std::env::var(ENV_GATEWAY_LAUNCH_LOCK_STALE_SECS)
        .ok()
        .and_then(|raw| raw.parse::<u64>().ok())
        .filter(|secs| *secs > 0)
        .map(Duration::from_secs)
        .unwrap_or_else(|| Duration::from_secs(DEFAULT_GATEWAY_LAUNCH_LOCK_STALE_SECS))
}

fn remove_stale_launch_lock(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    if !launch_lock_is_stale(path, stale_after)? {
        return Ok(false);
    }

    // Re-check immediately before unlinking so a fresh lock created by another
    // sidecar is not removed after the stale check wins a race.
    if !launch_lock_is_stale(path, stale_after)? {
        return Ok(false);
    }

    match std::fs::remove_file(path) {
        Ok(()) => Ok(true),
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => Ok(true),
        Err(err) => Err(err),
    }
}

fn launch_lock_is_stale(path: &Path, stale_after: Duration) -> std::io::Result<bool> {
    let modified = match std::fs::metadata(path) {
        Ok(meta) => meta.modified()?,
        Err(err) if err.kind() == std::io::ErrorKind::NotFound => return Ok(true),
        Err(err) => return Err(err),
    };
    let age = modified.elapsed().unwrap_or_default();
    Ok(age >= stale_after)
}

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

#[cfg(test)]
mod tests {
    use super::*;

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

    #[test]
    fn stale_gateway_launch_lock_is_reclaimed() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway-launch.lock");
        std::fs::write(&path, "stale").unwrap();

        let lock = acquire_launch_lock_with_stale(&path, Duration::ZERO)
            .expect("stale launch lock should be reclaimed");

        assert!(path.exists());
        drop(lock);
        assert!(!path.exists());
    }

    #[test]
    fn fresh_gateway_launch_lock_stays_single_flight() {
        let dir = tempfile::tempdir().unwrap();
        let path = dir.path().join("gateway-launch.lock");
        std::fs::write(&path, "busy").unwrap();

        let err = match acquire_launch_lock_with_stale(&path, Duration::from_secs(3600)) {
            Ok(_) => panic!("fresh launch lock should remain busy"),
            Err(err) => err,
        };

        assert_eq!(err.kind(), std::io::ErrorKind::AlreadyExists);
        assert!(path.exists());
    }
}
