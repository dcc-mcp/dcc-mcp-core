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

    /// Seconds to wait for the initial ``HostRpcClient::connect`` to the
    /// DCC. Failure to connect within this budget is logged but does
    /// **not** abort the sidecar — the process keeps running so its
    /// FileRegistry row is visible and the PPID-watch can still detect
    /// parent death. The gateway sees a registered-but-disconnected
    /// backend and routes around it.
    #[arg(long, value_name = "SECS", default_value = "10")]
    pub connect_timeout_secs: u64,

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

    // Instantiate the HostRpcClient impl for the URI's scheme and try
    // to dial the DCC. Failure to connect is logged but non-fatal —
    // the sidecar keeps running so PPID-watch / FileRegistry still
    // serve their purpose, and a future PR will add a reconnect loop
    // for transient DCC unavailability.
    let host_rpc_client = match dcc_mcp_host_rpc::client_for_uri(&args.host_rpc) {
        Ok(client) => {
            let connect_timeout = Duration::from_secs(args.connect_timeout_secs);
            match client_connect(client, &args.host_rpc, connect_timeout).await {
                Ok(connected) => {
                    tracing::info!(
                        host_rpc = %args.host_rpc,
                        "HostRpcClient connected"
                    );
                    Some(connected)
                }
                Err(err) => {
                    tracing::warn!(
                        host_rpc = %args.host_rpc,
                        error = %err,
                        "HostRpcClient connect failed; sidecar keeps running disconnected"
                    );
                    None
                }
            }
        }
        Err(err) => {
            tracing::error!(
                host_rpc = %args.host_rpc,
                error = %err,
                "unsupported --host-rpc scheme; sidecar keeps running but cannot dispatch"
            );
            None
        }
    };

    // Spin up the sidecar's own MCP HTTP listener so the gateway can
    // POST `tools/call` requests to us. If HostRpcClient connect
    // failed, we still start the listener — it returns structured
    // `transport-error` envelopes per call, which is much friendlier
    // than the gateway seeing a registered-but-unreachable backend.
    let mcp_handle = match host_rpc_client {
        Some(client) => {
            let state = crate::sidecar_mcp::SidecarMcpState::new(client, env!("CARGO_PKG_VERSION"));
            match crate::sidecar_mcp::spawn_listener(state, "127.0.0.1", 0).await {
                Ok(handle) => {
                    tracing::info!(
                        mcp_url = %handle.mcp_url,
                        "sidecar MCP listener up"
                    );
                    // Re-register the FileRegistry row so the gateway
                    // discovery surface includes our actual URL.
                    if let Err(err) = republish_mcp_url(&registry, &key, &handle) {
                        tracing::warn!(
                            error = %err,
                            "FileRegistry republish with mcp_url failed; gateway will route via stub"
                        );
                    }
                    Some(handle)
                }
                Err(err) => {
                    tracing::error!(error = %err, "sidecar MCP listener bind failed");
                    None
                }
            }
        }
        None => {
            tracing::warn!("skipping sidecar MCP listener — no HostRpcClient to dispatch through");
            None
        }
    };

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

    // Stop the HTTP listener first so the gateway sees the URL
    // disappear before we start tearing down the inner client. This
    // ordering also closes the HostRpcClient inside the listener's
    // state, so the DCC sees a clean disconnect.
    if let Some(handle) = mcp_handle {
        let state_close_url = handle.mcp_url.clone();
        handle.shutdown().await;
        tracing::info!(mcp_url = %state_close_url, "sidecar MCP listener stopped");
    }

    // Deregister is best-effort: a failure here would only leak a row that
    // will be reaped by the next stale-cleanup sweep, so we log and move on.
    if let Err(err) = registry.deregister(&key) {
        tracing::warn!(error = %err, "FileRegistry deregister failed");
    }

    Ok(())
}

/// Re-write the FileRegistry row with the live MCP URL once the
/// listener is bound. The original `register()` call happens before
/// the listener exists so the row carries a placeholder
/// `127.0.0.1:0` until this step runs — gateway discovery treats a
/// zero port as "registered but not yet routable" and avoids
/// dispatching to us during the brief startup window.
fn republish_mcp_url(
    registry: &Arc<FileRegistry>,
    key: &dcc_mcp_transport::discovery::types::ServiceKey,
    handle: &crate::sidecar_mcp::SidecarMcpListenerHandle,
) -> anyhow::Result<()> {
    let Some(mut entry) = registry.get(key) else {
        anyhow::bail!("registry row vanished before mcp_url republish")
    };
    entry.host = handle.bind_addr.ip().to_string();
    entry.port = handle.bind_addr.port();
    entry
        .metadata
        .insert("mcp_url".to_string(), handle.mcp_url.clone());
    // Deregister + register is atomic enough for our needs — the
    // FileRegistry only flushes after register() returns, so the
    // on-disk snapshot transitions in one step.
    registry.deregister(key)?;
    registry.register(entry)?;
    Ok(())
}

/// Connect the freshly-instantiated [`HostRpcClient`] to the DCC.
///
/// Wrapped as a separate helper so the caller can keep the `match`
/// arms in `run()` shallow and so the timeout / log surface is in
/// one place.
async fn client_connect(
    mut client: Box<dyn dcc_mcp_host_rpc::HostRpcClient>,
    endpoint: &str,
    timeout: Duration,
) -> Result<Box<dyn dcc_mcp_host_rpc::HostRpcClient>, dcc_mcp_host_rpc::HostRpcError> {
    client.connect(endpoint, timeout).await?;
    Ok(client)
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
            // Use the `stub` scheme so the HostRpcClient connects
            // immediately (no I/O) and the focus of this test stays
            // on the PPID-watch path. The commandport scheme is
            // exercised separately by `commandport_connects_to_fake_server`.
            host_rpc: "stub://localhost:0".to_string(),
            watch_pid: parent_pid,
            registry_dir: Some(registry_dir.path().to_path_buf()),
            instance_id: Some(Uuid::new_v4()),
            display_name: Some("test-sidecar".to_string()),
            adapter_version: Some("0.0.0-test".to_string()),
            connect_timeout_secs: 2,
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

    /// End-to-end commandport happy path: spawn a fake TCP server,
    /// spawn the sidecar with ``commandport://127.0.0.1:<port>``,
    /// assert the fake server observes the bootstrap line (proving
    /// the URI router picked CommandPortClient AND that connect()'s
    /// bootstrap-injection step ran), then kill the parent surrogate
    /// and assert clean exit.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn commandport_connects_to_fake_server() {
        use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
        use tokio::net::TcpListener;
        use tokio::sync::oneshot;

        let registry_dir = TempDir::new().expect("tempdir");

        // Bind a fake "Maya commandPort" on an OS-assigned port.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind 0");
        let port = listener.local_addr().expect("local_addr").port();
        let (connect_tx, connect_rx) = oneshot::channel::<()>();
        tokio::spawn(async move {
            // Accept one connection, reply to the bootstrap line, then
            // hold the socket open until teardown.
            if let Ok((mut stream, _)) = listener.accept().await {
                let _ = connect_tx.send(());
                let (read_half, mut write_half) = stream.split();
                let mut reader = BufReader::new(read_half);
                let mut bootstrap_line = String::new();
                let _ = reader.read_line(&mut bootstrap_line).await;
                // `exec()` evaluates to None in commandPort's reply path.
                let _ = write_half.write_all(b"None\n").await;
                let _ = write_half.flush().await;
                // Keep the socket alive until the sidecar tears down.
                // 5s is more than enough for this test's lifetime.
                tokio::time::sleep(Duration::from_secs(5)).await;
            }
        });

        let mut child = std::process::Command::new(sleep_cmd())
            .args(sleep_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep child");

        let parent_pid = child.id();
        let key_dcc = "maya".to_string();
        let pinned_uuid = Uuid::new_v4();
        let args = SidecarArgs {
            dcc: key_dcc.clone(),
            host_rpc: format!("commandport://127.0.0.1:{port}"),
            watch_pid: parent_pid,
            registry_dir: Some(registry_dir.path().to_path_buf()),
            instance_id: Some(pinned_uuid),
            display_name: Some("test-maya".to_string()),
            adapter_version: Some("0.0.0-test".to_string()),
            connect_timeout_secs: 2,
            ppid_poll_ms: Some(50),
        };

        let sidecar_handle = tokio::spawn(async move { run(args).await });

        // Confirm the sidecar's CommandPortClient actually connected
        // — this proves the URI router picked the right impl AND
        // that the connect() path is wired through end-to-end.
        tokio::time::timeout(Duration::from_secs(3), connect_rx)
            .await
            .expect("sidecar must connect to fake commandPort within 3s")
            .expect("connect channel closed without firing");

        // Confirm the registry row landed too (orthogonal to the
        // connect — the row is written before connect attempts).
        wait_for_registration(
            registry_dir.path(),
            &key_dcc,
            pinned_uuid,
            Duration::from_secs(2),
        )
        .await
        .expect("sidecar registered itself within 2s");

        // Kill the parent and assert clean shutdown.
        child.kill().expect("kill sleep child");
        let _ = child.wait();

        let result = tokio::time::timeout(Duration::from_secs(3), sidecar_handle)
            .await
            .expect("sidecar exited within 3s of parent death")
            .expect("sidecar task did not panic");
        result.expect("sidecar run returned ok");

        let registry = FileRegistry::new(registry_dir.path()).expect("reopen");
        let key = ServiceKey {
            dcc_type: key_dcc,
            instance_id: pinned_uuid,
        };
        assert!(
            registry.get(&key).is_none(),
            "sidecar must have deregistered itself"
        );
    }

    /// Soft-failure path: when the URI's host:port is dead, the sidecar
    /// logs a warning but **keeps running** so its FileRegistry row
    /// stays visible and PPID-watch can still detect parent death.
    /// The gateway sees a registered-but-disconnected backend and
    /// routes around it.
    #[tokio::test(flavor = "multi_thread", worker_threads = 2)]
    async fn sidecar_survives_failed_initial_connect() {
        use tokio::net::TcpListener;

        let registry_dir = TempDir::new().expect("tempdir");

        // Allocate a port and immediately drop the listener so any
        // connect attempt sees ECONNREFUSED quickly.
        let listener = TcpListener::bind("127.0.0.1:0").await.expect("bind");
        let dead_port = listener.local_addr().expect("local_addr").port();
        drop(listener);

        let mut child = std::process::Command::new(sleep_cmd())
            .args(sleep_args())
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .spawn()
            .expect("spawn sleep child");

        let parent_pid = child.id();
        let key_dcc = "maya".to_string();
        let pinned_uuid = Uuid::new_v4();
        let args = SidecarArgs {
            dcc: key_dcc.clone(),
            host_rpc: format!("commandport://127.0.0.1:{dead_port}"),
            watch_pid: parent_pid,
            registry_dir: Some(registry_dir.path().to_path_buf()),
            instance_id: Some(pinned_uuid),
            display_name: None,
            adapter_version: None,
            // 300ms is plenty for ECONNREFUSED on Windows; bumps any
            // slow CI well above the noise floor while keeping the
            // test snappy in the common case.
            connect_timeout_secs: 1,
            ppid_poll_ms: Some(50),
        };

        let sidecar_handle = tokio::spawn(async move { run(args).await });

        // Even with connect failed, the sidecar must register itself
        // — that's the whole point of the soft-failure contract.
        wait_for_registration(
            registry_dir.path(),
            &key_dcc,
            pinned_uuid,
            Duration::from_secs(3),
        )
        .await
        .expect("sidecar must register even when connect fails");

        child.kill().expect("kill sleep child");
        let _ = child.wait();

        let result = tokio::time::timeout(Duration::from_secs(4), sidecar_handle)
            .await
            .expect("sidecar exited after parent death")
            .expect("no panic");
        result.expect("run() returned ok");
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
