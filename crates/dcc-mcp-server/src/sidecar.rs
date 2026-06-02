//! `sidecar` subcommand — out-of-process worker for crash-isolated DCC actions.
//!
//! This is the **runtime substrate** for the sidecar epic (RFC #998).  The job
//! of a sidecar process is to:
//!
//! 1. Watch its **parent DCC** PID; exit cleanly when the parent dies so we
//!    never leak stale workers.
//! 2. Register itself in the shared `FileRegistry` with the
//!    `per-dcc-sidecar` role tag so the gateway can discover it.
//! 3. Hold a stable RPC channel to the live DCC through a registered
//!    `HostRpcClient` scheme such as `commandport://...`, `qtserver://...`,
//!    or `ws://...`.  When the DCC dies, the sidecar sees a transport
//!    disconnect — **not** a process death — and can emit a structured
//!    `host-died` envelope instead of cascading transport errors.
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
//! * **gateway daemon**: machine-wide singleton, lives in the same binary
//!   under the `gateway` subcommand. Per-DCC sidecars auto-launch it when
//!   needed and do not participate in gateway election by default.
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
//!     --host-rpc ws://127.0.0.1:7100/rpc \
//!     --watch-pid 67890
//! ```

use std::path::PathBuf;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::Context as _;
use clap::Args;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};
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

const FAILURE_REASON_METADATA_KEY: &str = "failure_reason";
const FAILURE_STAGE_METADATA_KEY: &str = "failure_stage";
const FAILURE_AT_UNIX_METADATA_KEY: &str = "failure_at_unix";
const HOST_RPC_URI_METADATA_KEY: &str = "host_rpc_uri";
const HOST_RPC_SCHEME_METADATA_KEY: &str = "host_rpc_scheme";
const DISPATCH_STATUS_METADATA_KEY: &str = "dispatch_status";
const DISPATCH_READY_AT_UNIX_METADATA_KEY: &str = "dispatch_ready_at_unix";
const DISPATCH_STATUS_BOOTING: &str = "booting";
const DISPATCH_STATUS_READY: &str = "ready";
const DISPATCH_STATUS_UNAVAILABLE: &str = "unavailable";

/// How often we re-check whether `--watch-pid` is still alive.
///
/// 250 ms balances "detect crash quickly" against "don't burn CPU polling".
const PPID_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Keep the per-DCC sidecar registry row fresh while the parent DCC is alive.
const SIDECAR_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

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
    /// * `qtserver://127.0.0.1:18765` — Qt in-process sidecar server
    /// * `ws://127.0.0.1:9000` — Photoshop UXP / Figma plugin
    /// * `stub://localhost` — tests only; connects but returns transport errors
    ///
    /// The scheme selects which registered `HostRpcClient` impl handles the
    /// connection. Unsupported schemes still leave a visible registry row with
    /// `dispatch_status=unavailable`, but the sidecar will not publish an MCP
    /// URL until host RPC is connected.
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

    #[cfg(feature = "gateway-daemon")]
    let gateway_guardian_handle = {
        let gateway_daemon_options = if args.gateway_port > 0 && !args.no_ensure_gateway {
            Some(build_gateway_daemon_options(&args, registry_dir.clone()))
        } else {
            None
        };

        if let Some(opts) = gateway_daemon_options.as_ref() {
            crate::gateway_daemon::ensure_gateway_running(opts)
                .await
                .with_context(|| "ensuring standalone gateway is running")?;
        }

        if should_start_gateway_daemon_guardian(&args) {
            gateway_daemon_options.clone().map(|opts| {
                crate::gateway_daemon::spawn_gateway_guardian(
                    opts,
                    crate::gateway_daemon::GatewayGuardianSettings::from_env(),
                )
            })
        } else {
            None
        }
    };

    #[cfg(not(feature = "gateway-daemon"))]
    let gateway_guardian_handle: Option<tokio::task::JoinHandle<()>> = {
        if args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election {
            tracing::warn!(
                port = args.gateway_port,
                "sidecar cannot auto-launch a gateway daemon because the binary was built without the gateway-daemon feature"
            );
        }
        None
    };

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
    let heartbeat_handle =
        spawn_sidecar_heartbeat(registry.clone(), key.clone(), SIDECAR_HEARTBEAT_INTERVAL);

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
                    let reason = format!("host-rpc connect to `{}` failed: {}", args.host_rpc, err);
                    tracing::warn!(
                        host_rpc = %args.host_rpc,
                        error = %err,
                        "HostRpcClient connect failed; sidecar keeps running disconnected"
                    );
                    if let Err(mark_err) =
                        mark_sidecar_boot_failure(&registry, &key, "host-rpc-connect", reason)
                    {
                        tracing::warn!(
                            error = %mark_err,
                            "FileRegistry failed to record sidecar host-rpc failure"
                        );
                    }
                    None
                }
            }
        }
        Err(err) => {
            let reason = format!("unsupported host-rpc URI `{}`: {}", args.host_rpc, err);
            tracing::error!(
                host_rpc = %args.host_rpc,
                error = %err,
                "unsupported --host-rpc scheme; sidecar keeps running but cannot dispatch"
            );
            if let Err(mark_err) =
                mark_sidecar_boot_failure(&registry, &key, "host-rpc-scheme", reason)
            {
                tracing::warn!(
                    error = %mark_err,
                    "FileRegistry failed to record sidecar host-rpc scheme failure"
                );
            }
            None
        }
    };

    // Spin up the sidecar's own MCP HTTP listener so the gateway can
    // POST `tools/call` requests to us. If HostRpcClient connect
    // failed, we still start the listener — it returns structured
    // `transport-error` envelopes per call, which is much friendlier
    // than the gateway seeing a registered-but-unreachable backend.
    let mut gateway_control: Option<crate::sidecar_gateway::SidecarGatewayControl> = None;

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
                    if args.legacy_gateway_election
                        && args.gateway_port > 0
                        && let Some(entry) = registry.get(&key)
                    {
                        match crate::sidecar_gateway::start_sidecar_gateway(
                            &args,
                            registry.clone(),
                            entry,
                        )
                        .await
                        {
                            Ok(ctrl) => gateway_control = ctrl,
                            Err(err) => {
                                tracing::error!(
                                    error = %err,
                                    "sidecar gateway election failed; MCP listener still up"
                                );
                            }
                        }
                    }
                    Some(handle)
                }
                Err(err) => {
                    let reason = format!("sidecar MCP listener bind failed: {err}");
                    tracing::error!(error = %err, "sidecar MCP listener bind failed");
                    if let Err(mark_err) =
                        mark_sidecar_boot_failure(&registry, &key, "mcp-listener-bind", reason)
                    {
                        tracing::warn!(
                            error = %mark_err,
                            "FileRegistry failed to record sidecar listener failure"
                        );
                    }
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

    if let Some(handle) = gateway_guardian_handle {
        handle.abort();
    }

    if let Some(ctrl) = gateway_control.take() {
        ctrl.shutdown().await;
    }

    // Stop the HTTP listener first so the gateway sees the URL
    // disappear before we start tearing down the inner client. This
    // ordering also closes the HostRpcClient inside the listener's
    // state, so the DCC sees a clean disconnect.
    if let Some(handle) = mcp_handle {
        let state_close_url = handle.mcp_url.clone();
        handle.shutdown().await;
        tracing::info!(mcp_url = %state_close_url, "sidecar MCP listener stopped");
    }

    heartbeat_handle.abort();

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
    entry.status = ServiceStatus::Available;
    entry
        .metadata
        .insert("mcp_url".to_string(), handle.mcp_url.clone());
    entry.metadata.insert(
        DISPATCH_STATUS_METADATA_KEY.to_string(),
        DISPATCH_STATUS_READY.to_string(),
    );
    entry.metadata.insert(
        DISPATCH_READY_AT_UNIX_METADATA_KEY.to_string(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    );
    entry.metadata.remove(FAILURE_REASON_METADATA_KEY);
    entry.metadata.remove(FAILURE_STAGE_METADATA_KEY);
    entry.metadata.remove(FAILURE_AT_UNIX_METADATA_KEY);
    // Deregister + register is atomic enough for our needs — the
    // FileRegistry only flushes after register() returns, so the
    // on-disk snapshot transitions in one step.
    registry.deregister(key)?;
    registry.register(entry)?;
    Ok(())
}

fn mark_sidecar_boot_failure(
    registry: &Arc<FileRegistry>,
    key: &dcc_mcp_transport::discovery::types::ServiceKey,
    stage: &str,
    reason: String,
) -> anyhow::Result<()> {
    let Some(mut entry) = registry.get(key) else {
        anyhow::bail!("registry row vanished before sidecar failure metadata update")
    };
    entry.status = ServiceStatus::Booting;
    entry.metadata.insert(
        DISPATCH_STATUS_METADATA_KEY.to_string(),
        DISPATCH_STATUS_UNAVAILABLE.to_string(),
    );
    entry.metadata.remove(DISPATCH_READY_AT_UNIX_METADATA_KEY);
    entry
        .metadata
        .insert(FAILURE_STAGE_METADATA_KEY.to_string(), stage.to_string());
    entry
        .metadata
        .insert(FAILURE_REASON_METADATA_KEY.to_string(), reason);
    entry.metadata.insert(
        FAILURE_AT_UNIX_METADATA_KEY.to_string(),
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs()
            .to_string(),
    );
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

#[cfg(feature = "gateway-daemon")]
fn build_gateway_daemon_options(
    args: &SidecarArgs,
    registry_dir: PathBuf,
) -> crate::gateway_daemon::EnsureGatewayOptions {
    let gateway_host = args
        .gateway_host
        .clone()
        .unwrap_or_else(|| args.host.clone());
    crate::gateway_daemon::EnsureGatewayOptions {
        host: gateway_host,
        port: args.gateway_port,
        name: args.gateway_name.clone().or_else(|| {
            args.display_name
                .as_ref()
                .map(|name| format!("gateway-for-{name}"))
        }),
        registry_dir,
        remote_host: args.gateway_remote_host.clone(),
        remote_port: args.gateway_remote_port,
    }
}

#[cfg(feature = "gateway-daemon")]
fn should_start_gateway_daemon_guardian(args: &SidecarArgs) -> bool {
    args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election
}

fn build_service_entry(args: &SidecarArgs) -> ServiceEntry {
    // The sidecar starts as Booting with a placeholder port. Once the MCP
    // listener binds, `republish_mcp_url` swaps in the real endpoint. If the
    // HostRpc connection/listener fails, the row stays Booting with
    // `failure_reason` metadata so operators can diagnose it in Admin.
    let mut entry = ServiceEntry::new(&args.dcc, "127.0.0.1", 0).with_pid(args.watch_pid);
    entry.status = ServiceStatus::Booting;

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
        .insert(HOST_RPC_URI_METADATA_KEY.to_string(), args.host_rpc.clone());
    if let Ok(scheme) = dcc_mcp_host_rpc::parse_scheme(&args.host_rpc) {
        entry
            .metadata
            .insert(HOST_RPC_SCHEME_METADATA_KEY.to_string(), scheme);
    }
    entry.metadata.insert(
        DISPATCH_STATUS_METADATA_KEY.to_string(),
        DISPATCH_STATUS_BOOTING.to_string(),
    );
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

fn spawn_sidecar_heartbeat(
    registry: Arc<FileRegistry>,
    key: dcc_mcp_transport::discovery::types::ServiceKey,
    interval: Duration,
) -> tokio::task::JoinHandle<()> {
    tokio::spawn(async move {
        loop {
            tokio::time::sleep(interval).await;
            match registry.heartbeat(&key) {
                Ok(true) => {}
                Ok(false) => {
                    tracing::warn!(
                        dcc = %key.dcc_type,
                        instance_id = %key.instance_id,
                        "sidecar heartbeat skipped because registry row is missing"
                    );
                }
                Err(err) => {
                    tracing::warn!(
                        dcc = %key.dcc_type,
                        instance_id = %key.instance_id,
                        error = %err,
                        "sidecar heartbeat failed"
                    );
                }
            }
        }
    })
}

fn default_registry_dir() -> PathBuf {
    // Must match ``GatewayRunner::new``'s fallback exactly:
    //     std::env::temp_dir().join("dcc-mcp-registry")
    //
    // Previously this used ``<tempdir>/dcc-mcp/registry/`` (extra dir
    // level), which split-brained the FileRegistry whenever an in-DCC
    // adapter spawned a sidecar without explicitly forwarding
    // ``--registry-dir``: the sidecar wrote rows to one path while the
    // adapter's gateway runner read from another, so gateway election
    // saw only its own candidates. Observed on 2026-05-16 in a live
    // three-Maya session: 36 stale sidecar rows accumulated in the
    // wrong dir, gateway port stayed dark despite all peers alive
    // (see dcc-mcp-maya #248 follow-up commit a6e4dea7).
    //
    // RFC #998 follow-up. Aligned with:
    //   - ``crates/dcc-mcp-gateway/src/gateway/runner.rs::GatewayRunner::new``
    //   - ``python/dcc_mcp_core/server_base.py`` defaults
    //   - ``crates/dcc-mcp-server/src/main.rs`` (the non-sidecar paths)
    //
    // The env var ``DCC_MCP_REGISTRY_DIR`` always wins so deployments
    // pinning an explicit path (CI, multi-host, custom temp policy)
    // keep working.
    if let Ok(dir) = std::env::var("DCC_MCP_REGISTRY_DIR") {
        return PathBuf::from(dir);
    }
    std::env::temp_dir().join("dcc-mcp-registry")
}

// ── tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::ServiceKey;
    use std::process::Stdio;
    use std::sync::Mutex;
    use std::time::Instant;
    use tempfile::TempDir;

    // ── Regression: ``default_registry_dir`` must match GatewayRunner's ──

    static REGISTRY_ENV_LOCK: Mutex<()> = Mutex::new(());

    #[cfg(feature = "gateway-daemon")]
    fn guardian_test_args() -> SidecarArgs {
        SidecarArgs {
            dcc: "maya".to_string(),
            host_rpc: "stub://localhost:0".to_string(),
            watch_pid: std::process::id(),
            registry_dir: None,
            instance_id: Some(Uuid::nil()),
            display_name: Some("Maya-Test".to_string()),
            adapter_version: Some("0.0.0-test".to_string()),
            connect_timeout_secs: 2,
            ppid_poll_ms: Some(50),
            gateway_port: 9765,
            no_ensure_gateway: false,
            legacy_gateway_election: false,
            host: "127.0.0.1".to_string(),
            gateway_host: None,
            gateway_name: None,
            gateway_remote_host: "0.0.0.0".to_string(),
            gateway_remote_port: 59765,
        }
    }

    #[test]
    fn default_registry_dir_matches_gateway_runner_fallback() {
        let _guard = REGISTRY_ENV_LOCK.lock().expect("registry env lock");
        // ``GatewayRunner::new`` (crates/dcc-mcp-gateway/src/gateway/
        // runner.rs) falls back to ``std::env::temp_dir().join("dcc-mcp-
        // registry")``. The sidecar binary MUST agree, otherwise an
        // adapter that spawns a sidecar without forwarding
        // ``--registry-dir`` will split-brain the registry.
        //
        // Wipe ``DCC_MCP_REGISTRY_DIR`` for this assertion so we hit the
        // fallback path (the env-var path is tested separately below).
        // Other parallel tests may also touch the env, but the value is
        // restored at the end so the suite stays clean.
        let saved = std::env::var("DCC_MCP_REGISTRY_DIR").ok();
        // SAFETY: single-threaded mutation guarded by ``saved``/restore
        // immediately after the call. Other tests in this file that
        // touch ``DCC_MCP_REGISTRY_DIR`` would have set their own values
        // and we don't disturb those.
        unsafe { std::env::remove_var("DCC_MCP_REGISTRY_DIR") };

        let got = default_registry_dir();
        let expected = std::env::temp_dir().join("dcc-mcp-registry");

        if let Some(prev) = saved {
            unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", prev) };
        }

        assert_eq!(
            got, expected,
            "sidecar default_registry_dir must match GatewayRunner::new \
             fallback (<tempdir>/dcc-mcp-registry). Mismatch split-brains \
             the FileRegistry and produces a dark gateway port."
        );
    }

    #[test]
    fn default_registry_dir_honours_env_var_override() {
        let _guard = REGISTRY_ENV_LOCK.lock().expect("registry env lock");
        let saved = std::env::var("DCC_MCP_REGISTRY_DIR").ok();
        let custom = std::env::temp_dir().join("dcc-mcp-custom-registry-test");
        unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", &custom) };

        let got = default_registry_dir();

        if let Some(prev) = saved {
            unsafe { std::env::set_var("DCC_MCP_REGISTRY_DIR", prev) };
        } else {
            unsafe { std::env::remove_var("DCC_MCP_REGISTRY_DIR") };
        }

        assert_eq!(
            got, custom,
            "DCC_MCP_REGISTRY_DIR must win over the fallback path"
        );
    }

    #[cfg(feature = "gateway-daemon")]
    #[test]
    fn gateway_daemon_guardian_runs_only_in_daemon_backed_mode() {
        let mut args = guardian_test_args();
        assert!(
            should_start_gateway_daemon_guardian(&args),
            "default sidecar mode should keep a daemon guardian alive"
        );

        args.gateway_port = 0;
        assert!(
            !should_start_gateway_daemon_guardian(&args),
            "gateway_port=0 explicitly disables gateway participation"
        );

        args.gateway_port = 9765;
        args.no_ensure_gateway = true;
        assert!(
            !should_start_gateway_daemon_guardian(&args),
            "--no-ensure-gateway opts out of daemon launch and guardian"
        );

        args.no_ensure_gateway = false;
        args.legacy_gateway_election = true;
        assert!(
            !should_start_gateway_daemon_guardian(&args),
            "legacy embedded election already owns its own probe loop"
        );
    }

    #[cfg(feature = "gateway-daemon")]
    #[test]
    fn gateway_daemon_options_preserve_host_name_and_registry() {
        let mut args = guardian_test_args();
        args.gateway_host = Some("0.0.0.0".to_string());
        args.gateway_name = Some("studio-gateway".to_string());
        let registry_dir = PathBuf::from("/tmp/dcc-mcp-registry-test");

        let opts = build_gateway_daemon_options(&args, registry_dir.clone());
        assert_eq!(opts.host, "0.0.0.0");
        assert_eq!(opts.name.as_deref(), Some("studio-gateway"));
        assert_eq!(opts.registry_dir, registry_dir);
        assert_eq!(opts.remote_host, "0.0.0.0");
        assert_eq!(opts.remote_port, 59765);

        let mut display_name_args = guardian_test_args();
        display_name_args.display_name = Some("Blender-Lookdev".to_string());
        let opts = build_gateway_daemon_options(&display_name_args, PathBuf::from("registry"));
        assert_eq!(opts.host, "127.0.0.1");
        assert_eq!(opts.name.as_deref(), Some("gateway-for-Blender-Lookdev"));
    }

    #[tokio::test]
    async fn sidecar_heartbeat_keeps_registry_row_fresh() {
        let registry_dir = TempDir::new().expect("tempdir");
        let registry = Arc::new(FileRegistry::new(registry_dir.path()).expect("registry"));
        let entry = ServiceEntry::new("3dsmax", "127.0.0.1", 55201).with_pid(std::process::id());
        let key = entry.key();
        registry.register(entry).expect("register sidecar row");
        let before = registry.get(&key).expect("registered row").last_heartbeat;

        let handle =
            spawn_sidecar_heartbeat(registry.clone(), key.clone(), Duration::from_millis(10));
        tokio::time::sleep(Duration::from_millis(40)).await;
        handle.abort();

        let after = registry.get(&key).expect("heartbeat row").last_heartbeat;
        assert!(
            after > before,
            "sidecar heartbeat must advance while the sidecar process is alive"
        );
    }

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
            gateway_port: 0,
            no_ensure_gateway: false,
            legacy_gateway_election: false,
            host: "127.0.0.1".to_string(),
            gateway_host: None,
            gateway_name: None,
            gateway_remote_host: "0.0.0.0".to_string(),
            gateway_remote_port: 59765,
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
            gateway_port: 0,
            no_ensure_gateway: false,
            legacy_gateway_election: false,
            host: "127.0.0.1".to_string(),
            gateway_host: None,
            gateway_name: None,
            gateway_remote_host: "0.0.0.0".to_string(),
            gateway_remote_port: 59765,
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
        let ready_row = wait_for_dispatch_status(
            registry_dir.path(),
            &key_dcc,
            pinned_uuid,
            DISPATCH_STATUS_READY,
            Duration::from_secs(3),
        )
        .await
        .expect("sidecar must publish dispatch-ready metadata");
        assert_eq!(ready_row.status, ServiceStatus::Available);
        assert_ne!(ready_row.port, 0);
        assert_eq!(
            ready_row
                .metadata
                .get(HOST_RPC_SCHEME_METADATA_KEY)
                .map(String::as_str),
            Some("commandport")
        );
        assert!(ready_row.metadata.contains_key("mcp_url"));
        assert!(
            ready_row
                .metadata
                .contains_key(DISPATCH_READY_AT_UNIX_METADATA_KEY),
            "dispatch-ready row should include a timestamp"
        );

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
            gateway_port: 0,
            no_ensure_gateway: false,
            legacy_gateway_election: false,
            host: "127.0.0.1".to_string(),
            gateway_host: None,
            gateway_name: None,
            gateway_remote_host: "0.0.0.0".to_string(),
            gateway_remote_port: 59765,
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
        let failed_row = wait_for_failure_reason(
            registry_dir.path(),
            &key_dcc,
            pinned_uuid,
            Duration::from_secs(3),
        )
        .await
        .expect("sidecar should expose host-rpc failure metadata");
        assert_eq!(failed_row.status, ServiceStatus::Booting);
        assert_eq!(failed_row.port, 0);
        assert_eq!(
            failed_row
                .metadata
                .get(HOST_RPC_SCHEME_METADATA_KEY)
                .map(String::as_str),
            Some("commandport")
        );
        assert_eq!(
            failed_row
                .metadata
                .get(DISPATCH_STATUS_METADATA_KEY)
                .map(String::as_str),
            Some(DISPATCH_STATUS_UNAVAILABLE)
        );
        assert_eq!(
            failed_row
                .metadata
                .get(FAILURE_STAGE_METADATA_KEY)
                .map(String::as_str),
            Some("host-rpc-connect")
        );
        assert!(
            failed_row
                .metadata
                .get(FAILURE_REASON_METADATA_KEY)
                .is_some_and(|reason| reason.contains("host-rpc connect"))
        );

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

    async fn wait_for_dispatch_status(
        registry_dir: &std::path::Path,
        dcc: &str,
        instance_id: Uuid,
        expected: &str,
        timeout: Duration,
    ) -> anyhow::Result<ServiceEntry> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                anyhow::bail!("registry row never reached dispatch_status={expected}");
            }
            let registry =
                FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
            let key = ServiceKey {
                dcc_type: dcc.to_string(),
                instance_id,
            };
            if let Some(row) = registry.get(&key)
                && row
                    .metadata
                    .get(DISPATCH_STATUS_METADATA_KEY)
                    .is_some_and(|status| status == expected)
            {
                return Ok(row);
            }
            tokio::time::sleep(Duration::from_millis(25)).await;
        }
    }

    async fn wait_for_failure_reason(
        registry_dir: &std::path::Path,
        dcc: &str,
        instance_id: Uuid,
        timeout: Duration,
    ) -> anyhow::Result<ServiceEntry> {
        let deadline = Instant::now() + timeout;
        loop {
            if Instant::now() >= deadline {
                anyhow::bail!("registry row never recorded failure metadata");
            }
            let registry =
                FileRegistry::new(registry_dir).with_context(|| "reopen registry while polling")?;
            let key = ServiceKey {
                dcc_type: dcc.to_string(),
                instance_id,
            };
            if let Some(row) = registry.get(&key)
                && row.metadata.contains_key(FAILURE_REASON_METADATA_KEY)
            {
                return Ok(row);
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
