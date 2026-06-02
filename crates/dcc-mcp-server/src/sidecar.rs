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
    /// `dispatch_status=unavailable`; the sidecar may still publish a
    /// diagnostic MCP URL that returns structured transport errors instead of
    /// becoming routable.
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

    /// Test hook: allow `stub://` to publish dispatch-ready metadata.
    ///
    /// Production launchers must use a real host RPC scheme. Without this
    /// explicit opt-in, `stub://` remains a diagnostic listener so installers
    /// cannot mistake a test placeholder for a callable DCC dispatcher.
    #[arg(long, hide = true, default_value = "false")]
    pub allow_stub_dispatch_ready: bool,

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
        let gateway_daemon_options = if should_use_gateway_daemon(&args) {
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
    let (host_rpc_client, dispatch_ready): (Box<dyn dcc_mcp_host_rpc::HostRpcClient>, bool) =
        match dcc_mcp_host_rpc::client_for_uri(&args.host_rpc) {
            Ok(client) => {
                let connect_timeout = Duration::from_secs(args.connect_timeout_secs);
                match client_connect(client, &args.host_rpc, connect_timeout).await {
                    Ok(connected) => {
                        if connected.uri_scheme() == "stub" && !args.allow_stub_dispatch_ready {
                            let reason = format!(
                                "host-rpc URI `{}` uses stub://, which is test-only and cannot prove DCC dispatch readiness",
                                args.host_rpc
                            );
                            tracing::warn!(
                                host_rpc = %args.host_rpc,
                                "stub HostRpcClient connected; sidecar remains unavailable unless --allow-stub-dispatch-ready is set"
                            );
                            if let Err(mark_err) = mark_sidecar_boot_failure(
                                &registry,
                                &key,
                                "host-rpc-stub",
                                reason.clone(),
                            ) {
                                tracing::warn!(
                                    error = %mark_err,
                                    "FileRegistry failed to record sidecar stub host-rpc failure"
                                );
                            }
                            (
                                Box::new(dcc_mcp_host_rpc::UnavailableHostRpcClient::new(reason)),
                                false,
                            )
                        } else {
                            tracing::info!(
                                host_rpc = %args.host_rpc,
                                "HostRpcClient connected"
                            );
                            (connected, true)
                        }
                    }
                    Err(err) => {
                        let reason =
                            format!("host-rpc connect to `{}` failed: {}", args.host_rpc, err);
                        tracing::warn!(
                            host_rpc = %args.host_rpc,
                            error = %err,
                            "HostRpcClient connect failed; sidecar keeps running disconnected"
                        );
                        if let Err(mark_err) = mark_sidecar_boot_failure(
                            &registry,
                            &key,
                            "host-rpc-connect",
                            reason.clone(),
                        ) {
                            tracing::warn!(
                                error = %mark_err,
                                "FileRegistry failed to record sidecar host-rpc failure"
                            );
                        }
                        (
                            Box::new(dcc_mcp_host_rpc::UnavailableHostRpcClient::new(reason)),
                            false,
                        )
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
                    mark_sidecar_boot_failure(&registry, &key, "host-rpc-scheme", reason.clone())
                {
                    tracing::warn!(
                        error = %mark_err,
                        "FileRegistry failed to record sidecar host-rpc scheme failure"
                    );
                }
                (
                    Box::new(dcc_mcp_host_rpc::UnavailableHostRpcClient::new(reason)),
                    false,
                )
            }
        };

    // Spin up the sidecar's own MCP HTTP listener so the gateway can
    // POST `tools/call` requests to us. If HostRpcClient connect failed,
    // the listener still starts with an unavailable diagnostic client; the
    // registry row stays Booting/unavailable so gateway routing skips it, but
    // direct probes get a structured `transport-error` envelope.
    let mut gateway_control: Option<crate::sidecar_gateway::SidecarGatewayControl> = None;

    let state =
        crate::sidecar_mcp::SidecarMcpState::new(host_rpc_client, env!("CARGO_PKG_VERSION"));
    let mcp_handle = match crate::sidecar_mcp::spawn_listener(state, "127.0.0.1", 0).await {
        Ok(handle) => {
            tracing::info!(
                mcp_url = %handle.mcp_url,
                dispatch_ready,
                "sidecar MCP listener up"
            );
            // Re-register the FileRegistry row so the discovery/diagnostic
            // surface includes the actual URL. Only a dispatch-ready sidecar
            // becomes Available; unavailable rows keep Booting status so the
            // gateway live view does not route traffic to them.
            if let Err(err) = republish_mcp_listener(&registry, &key, &handle, dispatch_ready) {
                tracing::warn!(
                    error = %err,
                    "FileRegistry republish with mcp_url failed; gateway will route via stub"
                );
            }
            if dispatch_ready
                && args.legacy_gateway_election
                && args.gateway_port > 0
                && let Some(entry) = registry.get(&key)
            {
                match crate::sidecar_gateway::start_sidecar_gateway(&args, registry.clone(), entry)
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

/// Re-write the FileRegistry row with the live MCP URL once the listener is
/// bound. The original `register()` call happens before the listener exists so
/// the row carries a placeholder `127.0.0.1:0` until this step runs.
///
/// Dispatch-ready sidecars become `Available`; diagnostic listeners keep
/// `Booting` plus `dispatch_status=unavailable` so gateway discovery can show
/// the URL for operators without routing calls through it.
fn republish_mcp_listener(
    registry: &Arc<FileRegistry>,
    key: &dcc_mcp_transport::discovery::types::ServiceKey,
    handle: &crate::sidecar_mcp::SidecarMcpListenerHandle,
    dispatch_ready: bool,
) -> anyhow::Result<()> {
    let Some(mut entry) = registry.get(key) else {
        anyhow::bail!("registry row vanished before mcp_url republish")
    };
    entry.host = handle.bind_addr.ip().to_string();
    entry.port = handle.bind_addr.port();
    entry
        .metadata
        .insert("mcp_url".to_string(), handle.mcp_url.clone());
    if dispatch_ready {
        entry.status = ServiceStatus::Available;
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
    } else {
        entry.status = ServiceStatus::Booting;
        entry.metadata.insert(
            DISPATCH_STATUS_METADATA_KEY.to_string(),
            DISPATCH_STATUS_UNAVAILABLE.to_string(),
        );
        entry.metadata.remove(DISPATCH_READY_AT_UNIX_METADATA_KEY);
    }
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
    should_use_gateway_daemon(args)
}

fn should_use_gateway_daemon(args: &SidecarArgs) -> bool {
    args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election
}

fn build_service_entry(args: &SidecarArgs) -> ServiceEntry {
    // The sidecar starts as Booting with a placeholder port. Once the MCP
    // listener binds, `republish_mcp_listener` swaps in the real endpoint. If
    // the HostRpc connection fails, the row still gets a diagnostic MCP URL but
    // stays Booting/unavailable so operators can diagnose it in Admin without
    // making it routable.
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
#[path = "sidecar_tests.rs"]
mod sidecar_tests;
