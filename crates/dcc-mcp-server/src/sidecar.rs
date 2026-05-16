//! `sidecar` subcommand — out-of-process worker for crash-isolated DCC actions.
//!
//! This is the **runtime substrate** for the sidecar epic (RFC #998).  The job
//! of a sidecar process is to:
//!
//! 1. Watch its **parent DCC** PID; exit cleanly when the parent dies so we
//!    never leak stale workers.
//! 2. Register itself in the shared `FileRegistry` with the
//!    `per-dcc-sidecar` role tag so the gateway can discover it.
//! 3. Hold a stable RPC channel to the live DCC (`commandPort://...`,
//!    `hrpyc://...`, `http://...`).  When the DCC dies, the sidecar sees a
//!    transport disconnect — **not** a process death — and can emit a
//!    structured `host-died` envelope instead of cascading transport errors.
//! 4. On graceful shutdown (PPID death or `ctrl-c`), deregister from
//!    `FileRegistry` and drop the OS-held sentinel lock.
//!
//! Per-DCC `HostRpcClient` implementations live in `dcc-mcp-host-rpc`
//! (separate crate, also new); this module only wires the lifecycle.
//!
//! # Two-tier sidecar model (RFC #998 Addendum A.1)
//!
//! * **per-DCC sidecar**: one per DCC, lifetime bound to DCC's PID.  This
//!   subcommand is the per-DCC sidecar.  The `--watch-pid` flag is the
//!   parent-DCC PID; this process is **not** detached.
//! * **gateway sidecar**: machine-wide singleton, lives in the same binary
//!   but runs *without* `sidecar` subcommand (i.e. the default
//!   `dcc-mcp-server` invocation that wins the `bind(9765)` race).  Existing
//!   gateway logic is unchanged.
//!
//! # Example
//!
//! ```bash
//! # Maya plugin spawns this:
//! dcc-mcp-server sidecar \
//!     --dcc maya \
//!     --host-rpc commandport://127.0.0.1:6000 \
//!     --watch-pid 12345 \
//!     --registry-dir ~/.cache/dcc-mcp/registry
//!
//! # Blender addon spawns this:
//! dcc-mcp-server sidecar \
//!     --dcc blender \
//!     --host-rpc http://127.0.0.1:7100/rpc \
//!     --watch-pid 67890
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use clap::Args;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::ServiceEntry;
use tokio::sync::watch;
use uuid::Uuid;

use crate::is_process_alive;

/// FileRegistry `metadata` key used to tag sidecar rows.
///
/// Values are one of:
/// * `"per-dcc-sidecar"` — a sidecar child of a single DCC process
/// * `"gateway-sidecar"` — the machine-wide gateway sidecar (set elsewhere,
///   not by this subcommand)
pub const ROLE_METADATA_KEY: &str = "dcc_mcp_role";

/// Value stored in `metadata[ROLE_METADATA_KEY]` for per-DCC sidecars.
pub const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";

/// How often we re-check whether `--watch-pid` is still alive.
///
/// 250 ms balances "detect crash quickly" against "don't burn CPU polling".
const PPID_POLL_INTERVAL: Duration = Duration::from_millis(250);

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
    /// * `commandport://127.0.0.1:6000` — Maya `commandPort`
    /// * `hrpyc://127.0.0.1:18811` — Houdini `hrpyc`
    /// * `http://127.0.0.1:7100/rpc` — Blender custom JSON-RPC listener
    /// * `ws://127.0.0.1:9000` — Photoshop UXP / Figma plugin
    ///
    /// The scheme selects which `HostRpcClient` impl handles the connection.
    /// The actual client wiring is **not** implemented in this MVP — the
    /// sidecar registers and waits; tools/call returns a structured
    /// `not-implemented` envelope until per-DCC `HostRpcClient` impls land.
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

    /// Override the polling interval for PPID watch (test hook).
    #[arg(long, value_name = "MS", hide = true)]
    pub ppid_poll_ms: Option<u64>,
}

/// Run the sidecar lifecycle until the parent DCC dies or a signal arrives.
pub async fn run(args: SidecarArgs) -> anyhow::Result<()> {
    tracing::info!(
        dcc = %args.dcc,
        host_rpc = %args.host_rpc,
        watch_pid = args.watch_pid,
        "dcc-mcp-server sidecar starting"
    );

    let registry_dir = args
        .registry_dir
        .clone()
        .unwrap_or_else(default_registry_dir);
    std::fs::create_dir_all(&registry_dir)
        .with_context(|| format!("creating registry dir {}", registry_dir.display()))?;

    let registry = Arc::new(
        FileRegistry::new(&registry_dir)
            .with_context(|| format!("opening FileRegistry at {}", registry_dir.display()))?,
    );

    let entry = build_service_entry(&args);
    let key = entry.key();

    registry
        .register(entry)
        .with_context(|| "registering sidecar in FileRegistry")?;
    tracing::info!(
        instance_id = %key.instance_id,
        dcc = %key.dcc_type,
        "sidecar registered"
    );

    // PPID-watch lives on its own task; on parent-death it flips the
    // `exit_tx` watch channel.  Same channel is used by the ctrl-c branch
    // below, so both paths converge on the same deregister flow.
    let (exit_tx, mut exit_rx) = watch::channel::<Option<ExitReason>>(None);
    spawn_ppid_watcher(
        args.watch_pid,
        Duration::from_millis(
            args.ppid_poll_ms
                .unwrap_or(PPID_POLL_INTERVAL.as_millis() as u64),
        ),
        exit_tx.clone(),
    );

    let reason = tokio::select! {
        _ = exit_rx.changed() => {
            exit_rx.borrow().unwrap_or(ExitReason::ParentDied)
        }
        sig = crate::select_shutdown_signal() => {
            let signal_name = sig.unwrap_or("unknown");
            tracing::info!(signal = signal_name, "sidecar received shutdown signal");
            ExitReason::Signal
        }
    };

    tracing::info!(reason = ?reason, "sidecar shutting down");

    // Deregister is best-effort: a failure here would only leak a row that
    // will be reaped by the next stale-cleanup sweep, so we log and move on.
    if let Err(err) = registry.deregister(&key) {
        tracing::warn!(error = %err, "FileRegistry deregister failed");
    }

    Ok(())
}

fn build_service_entry(args: &SidecarArgs) -> ServiceEntry {
    // The sidecar publishes its own host:port (currently `127.0.0.1:0`
    // because we haven't started an MCP HTTP listener in this MVP).
    // Once the HostRpcClient wiring lands and the sidecar speaks MCP HTTP
    // back to the gateway, this becomes its real bind address.
    let mut entry = ServiceEntry::new(&args.dcc, "127.0.0.1", 0).with_pid(args.watch_pid);

    if let Some(uuid) = args.instance_id {
        entry.instance_id = uuid;
    }
    if let Some(ref name) = args.display_name {
        entry.display_name = Some(name.clone());
    }
    if let Some(ref ver) = args.adapter_version {
        entry.adapter_version = Some(ver.clone());
        entry.adapter_dcc = Some(args.dcc.clone());
    }

    entry.metadata.insert(
        ROLE_METADATA_KEY.to_string(),
        ROLE_PER_DCC_SIDECAR.to_string(),
    );
    entry
        .metadata
        .insert("host_rpc_uri".to_string(), args.host_rpc.clone());
    entry
        .metadata
        .insert("sidecar_pid".to_string(), std::process::id().to_string());

    entry
}

fn spawn_ppid_watcher(
    parent_pid: u32,
    poll_interval: Duration,
    exit_tx: watch::Sender<Option<ExitReason>>,
) {
    tokio::spawn(async move {
        loop {
            if !is_process_alive(parent_pid) {
                tracing::info!(
                    parent_pid,
                    "parent DCC process no longer alive — signalling sidecar exit"
                );
                let _ = exit_tx.send(Some(ExitReason::ParentDied));
                return;
            }
            tokio::time::sleep(poll_interval).await;
        }
    });
}

fn default_registry_dir() -> PathBuf {
    // Mirror the discovery default used by `dcc-mcp-server` proper: the
    // env var wins; otherwise the OS temp dir.  Kept simple here because
    // the canonical path computation lives in `dcc-mcp-paths`, which is
    // already pulled in transitively but not directly depended on here.
    if let Ok(dir) = std::env::var("DCC_MCP_REGISTRY_DIR") {
        return PathBuf::from(dir);
    }
    let mut dir = std::env::temp_dir();
    dir.push("dcc-mcp");
    dir.push("registry");
    dir
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceKey;
    use std::process::Stdio;
    use std::time::Instant;
    use tempfile::TempDir;

    /// PPID-watch happy path: spawn a real child process, register a sidecar
    /// pinned to that child's PID, kill the child, assert the sidecar exits
    /// quickly and the FileRegistry row is gone.
    ///
    /// Uses a real OS process (not the current pid) to avoid the "watch_pid
    /// is the sidecar itself" footgun.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn ppid_watch_exits_on_parent_death() {
        let registry_dir = TempDir::new().expect("tempdir");

        // Spawn a long-sleeping child; we'll kill it to simulate DCC death.
        let mut child = std::process::Command::new(sleep_cmd())
            .args(sleep_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep child");

        let parent_pid = child.id();
        let key_dcc = "test-dcc".to_string();
        let args = SidecarArgs {
            dcc: key_dcc.clone(),
            host_rpc: "test://localhost:0".to_string(),
            watch_pid: parent_pid,
            registry_dir: Some(registry_dir.path().to_path_buf()),
            instance_id: Some(Uuid::new_v4()),
            display_name: Some("test-sidecar".to_string()),
            adapter_version: Some("0.0.0-test".to_string()),
            ppid_poll_ms: Some(50),
        };
        let pinned_uuid = args.instance_id.unwrap();

        // Run the sidecar in the background; it should register itself,
        // then exit shortly after we kill the parent.
        let sidecar_handle = tokio::spawn(async move { run(args).await });

        // Wait for registration to land before killing the parent — gives
        // the sidecar a fair shot at writing to FileRegistry.
        wait_for_registration(
            registry_dir.path(),
            &key_dcc,
            pinned_uuid,
            Duration::from_secs(2),
        )
        .await
        .expect("sidecar registered itself within 2s");

        // Kill the parent.
        child.kill().expect("kill sleep child");
        let _ = child.wait();

        // Sidecar should exit within ~250ms of detecting parent death
        // (50ms poll + a couple of ticks of slack on slow CI).
        let exit_deadline = Instant::now() + Duration::from_secs(3);
        let result = tokio::time::timeout_at(
            tokio::time::Instant::from_std(exit_deadline),
            sidecar_handle,
        )
        .await
        .expect("sidecar did not exit within 3s of parent death")
        .expect("sidecar task did not panic");
        result.expect("sidecar run returned an error");

        // FileRegistry row must be gone (deregister ran in the shutdown path).
        let registry = FileRegistry::new(registry_dir.path()).expect("reopen registry");
        let key = ServiceKey {
            dcc_type: key_dcc,
            instance_id: pinned_uuid,
        };
        assert!(
            registry.get(&key).is_none(),
            "sidecar should have deregistered itself; row still present"
        );
    }

    fn sleep_cmd() -> &'static str {
        if cfg!(windows) {
            "powershell.exe"
        } else {
            "sleep"
        }
    }

    fn sleep_args() -> Vec<&'static str> {
        if cfg!(windows) {
            vec!["-NoProfile", "-Command", "Start-Sleep -Seconds 60"]
        } else {
            vec!["60"]
        }
    }

    async fn wait_for_registration(
        registry_dir: &std::path::Path,
        dcc: &str,
        instance_id: Uuid,
        timeout: Duration,
    ) -> anyhow::Result<()> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                anyhow::bail!("registry row never appeared");
            }
            // Reopening the registry forces a reload from disk; the
            // background sidecar writes through `flush_to_file`.
            let registry =
                FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
            let key = ServiceKey {
                dcc_type: dcc.to_string(),
                instance_id,
            };
            if registry.get(&key).is_some() {
                return Ok(());
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    #[test]
    fn role_metadata_key_is_stable() {
        // Pin the public constant so downstream tools that grep for it
        // (admin UI / observability dashboards) cannot break silently.
        assert_eq!(ROLE_METADATA_KEY, "dcc_mcp_role");
        assert_eq!(ROLE_PER_DCC_SIDECAR, "per-dcc-sidecar");
    }
}
