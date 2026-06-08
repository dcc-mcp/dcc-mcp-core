//! `sidecar` subcommand - out-of-process worker for crash-isolated DCC actions.
//!
//! This is the **runtime substrate** for the sidecar epic (RFC #998). The job
//! of a sidecar process is to:
//!
//! 1. Watch its **parent DCC** PID; exit cleanly when the parent dies so we
//!    never leak stale workers.
//! 2. Register itself in the shared `FileRegistry` with the
//!    `per-dcc-sidecar` role tag so the gateway can discover it.
//! 3. Hold a stable RPC channel to the live DCC through a registered
//!    `HostRpcClient` scheme such as `commandport://...`, `qtserver://...`,
//!    or `ws://...`. When the DCC dies, the sidecar sees a transport
//!    disconnect and can emit a structured `host-died` envelope.
//! 4. On graceful shutdown (PPID death or `ctrl-c`), deregister from
//!    `FileRegistry` and drop the OS-held sentinel lock.
//!
//! Per-DCC `HostRpcClient` implementations live in `dcc-mcp-host-rpc`; this
//! module wires the lifecycle.
//!
//! # Two-tier sidecar model (RFC #998 Addendum A.1)
//!
//! * **per-DCC sidecar**: one per DCC, lifetime bound to DCC's PID. This
//!   subcommand is the per-DCC sidecar. The `--watch-pid` flag is the
//!   parent-DCC PID; this process is **not** detached.
//! * **gateway daemon**: machine-wide singleton, lives in the same binary
//!   under the `gateway` subcommand. Per-DCC sidecars auto-launch it when
//!   needed and do not participate in gateway election by default.
//!
//! # Example
//!
//! ```bash
//! dcc-mcp-server sidecar \
//!     --dcc maya \
//!     --host-rpc commandport://127.0.0.1:6000 \
//!     --watch-pid 12345 \
//!     --registry-dir ~/.cache/dcc-mcp/registry
//!
//! dcc-mcp-server sidecar \
//!     --dcc blender \
//!     --host-rpc ws://127.0.0.1:7100/rpc \
//!     --watch-pid 67890
//! ```

mod args;
mod gateway;
mod lifecycle;
mod registry;

use std::sync::Arc;
use std::time::Duration;

use anyhow::Context as _;
use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use tokio::sync::Mutex as AsyncMutex;
use tokio::sync::watch;

pub use args::{ExitReason, SidecarArgs};
pub use registry::{ROLE_METADATA_KEY, ROLE_PER_DCC_SIDECAR};

#[cfg(feature = "gateway-daemon")]
pub(crate) use gateway::{
    build_gateway_daemon_options, should_start_gateway_daemon_guardian, should_use_gateway_daemon,
};
#[cfg(feature = "gateway-daemon")]
use lifecycle::spawn_guardian_status_publisher;

/// Inner type for the shared guardian handle. With `gateway-daemon`, this is
/// the real [`crate::gateway_daemon::GatewayGuardianHandle`]; without it, we
/// use a plain `JoinHandle<()>` so the type resolves cleanly (the handle is
/// always `None` in that case).
#[cfg(feature = "gateway-daemon")]
type GuardianHandleInner = crate::gateway_daemon::GatewayGuardianHandle;
#[cfg(not(feature = "gateway-daemon"))]
type GuardianHandleInner = tokio::task::JoinHandle<()>;
use lifecycle::{
    client_connect, should_retry_host_rpc_connect, spawn_host_rpc_reconnector, spawn_ppid_watcher,
    spawn_sidecar_heartbeat,
};
use registry::{
    build_service_entry, default_registry_dir, mark_sidecar_boot_failure, republish_mcp_listener,
};

#[cfg(test)]
pub(crate) use registry::{
    DISPATCH_READY_AT_UNIX_METADATA_KEY, DISPATCH_STATUS_METADATA_KEY, DISPATCH_STATUS_READY,
    DISPATCH_STATUS_UNAVAILABLE, FAILURE_REASON_METADATA_KEY, FAILURE_STAGE_METADATA_KEY,
    GATEWAY_GUARDIAN_ENABLED_METADATA_KEY, GATEWAY_RUNTIME_MODE_METADATA_KEY,
    HOST_RPC_SCHEME_METADATA_KEY,
};

/// How often we re-check whether `--watch-pid` is still alive.
///
/// 250 ms balances "detect crash quickly" against "don't burn CPU polling".
const PPID_POLL_INTERVAL: Duration = Duration::from_millis(250);

/// Keep the per-DCC sidecar registry row fresh while the parent DCC is alive.
const SIDECAR_HEARTBEAT_INTERVAL: Duration = Duration::from_secs(5);

/// Retry interval for HostRpc endpoints that are not ready yet at DCC startup.
#[cfg(not(test))]
const HOST_RPC_RECONNECT_INTERVAL: Duration = Duration::from_secs(2);

/// Keep reconnect tests quick without changing production retry cadence.
#[cfg(test)]
const HOST_RPC_RECONNECT_INTERVAL: Duration = Duration::from_millis(50);

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
    let gateway_daemon_options = if should_use_gateway_daemon(&args) {
        Some(build_gateway_daemon_options(&args, registry_dir.clone()))
    } else {
        None
    };

    #[cfg(feature = "gateway-daemon")]
    if let Some(opts) = gateway_daemon_options.as_ref() {
        crate::gateway_daemon::ensure_gateway_running(opts)
            .await
            .with_context(|| "ensuring standalone gateway is running")?;
    }

    #[cfg(feature = "gateway-daemon")]
    let gateway_guardian_handle: Arc<AsyncMutex<Option<GuardianHandleInner>>> = {
        let guardian = if should_start_gateway_daemon_guardian(&args) {
            gateway_daemon_options.clone().map(|opts| {
                crate::gateway_daemon::spawn_gateway_guardian(
                    opts,
                    crate::gateway_daemon::GatewayGuardianSettings::from_env(),
                )
            })
        } else {
            None
        };
        Arc::new(AsyncMutex::new(guardian))
    };

    #[cfg(not(feature = "gateway-daemon"))]
    let gateway_guardian_handle: Arc<AsyncMutex<Option<GuardianHandleInner>>> = {
        if args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election {
            tracing::warn!(
                port = args.gateway_port,
                "sidecar cannot auto-launch a gateway daemon because the binary was built without the gateway-daemon feature"
            );
        }
        Arc::new(AsyncMutex::new(None))
    };

    // Spawn a watchdog that periodically polls the guardian's liveness and
    // restarts it automatically when it detects the guardian thread has died.
    #[cfg(feature = "gateway-daemon")]
    let gateway_guardian_watchdog: Option<tokio::task::JoinHandle<()>> = {
        let watchdog_opts = gateway_daemon_options.clone();
        let shared_handle = gateway_guardian_handle.clone();
        if watchdog_opts.is_some() {
            Some(tokio::spawn(async move {
                let mut interval = tokio::time::interval(Duration::from_secs(60));
                interval.set_missed_tick_behavior(tokio::time::MissedTickBehavior::Delay);
                loop {
                    interval.tick().await;
                    let should_restart = {
                        let locked = shared_handle.lock().await;
                        match locked.as_ref() {
                            Some(h) => !h.status().guardian_running,
                            None => false,
                        }
                    };
                    if should_restart {
                        tracing::warn!("Guardian watchdog detected dead guardian, restarting...");
                        if let Some(ref opts) = watchdog_opts {
                            let new_guardian = crate::gateway_daemon::spawn_gateway_guardian(
                                opts.clone(),
                                crate::gateway_daemon::GatewayGuardianSettings::from_env(),
                            );
                            *shared_handle.lock().await = Some(new_guardian);
                        }
                    }
                }
            }))
        } else {
            None
        }
    };
    #[cfg(not(feature = "gateway-daemon"))]
    let gateway_guardian_watchdog: Option<tokio::task::JoinHandle<()>> = None;

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

    #[cfg(feature = "gateway-daemon")]
    let gateway_guardian_publisher_handle: Option<tokio::task::JoinHandle<()>> = {
        let locked = gateway_guardian_handle.lock().await;
        locked.as_ref().map(|guardian| {
            // Spawn a lightweight publisher that periodically syncs the
            // guardian's live status into the sidecar's FileRegistry metadata
            // so admin UI and /v1/readyz can expose the fallback reason/state.
            spawn_guardian_status_publisher(
                guardian.clone(),
                registry.clone(),
                key.clone(),
                crate::gateway_daemon::GatewayGuardianSettings::from_env().interval(),
            )
        })
    };
    #[cfg(not(feature = "gateway-daemon"))]
    let gateway_guardian_publisher_handle: Option<tokio::task::JoinHandle<()>> = None;

    // Instantiate the HostRpcClient impl for the URI's scheme and try to dial
    // the DCC. Failure is non-fatal: the diagnostic listener stays up and a
    // background reconnect loop can promote the same row once the DCC-side
    // bridge starts accepting calls.
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

    let mut gateway_control: Option<crate::sidecar_gateway::SidecarGatewayControl> = None;

    let mut host_rpc_reconnect_handle: Option<tokio::task::JoinHandle<()>> = None;
    let state =
        crate::sidecar_mcp::SidecarMcpState::new(host_rpc_client, env!("CARGO_PKG_VERSION"));
    let reconnect_state = state.clone();
    let mcp_handle = match crate::sidecar_mcp::spawn_listener(state, "127.0.0.1", 0).await {
        Ok(handle) => {
            tracing::info!(
                mcp_url = %handle.mcp_url,
                dispatch_ready,
                "sidecar MCP listener up"
            );
            if let Err(err) = republish_mcp_listener(&registry, &key, &handle, dispatch_ready) {
                tracing::warn!(
                    error = %err,
                    "FileRegistry republish with mcp_url failed; gateway will route via stub"
                );
            }
            if !dispatch_ready && should_retry_host_rpc_connect(&args) {
                host_rpc_reconnect_handle = Some(spawn_host_rpc_reconnector(
                    reconnect_state,
                    registry.clone(),
                    key.clone(),
                    args.host_rpc.clone(),
                    Duration::from_secs(args.connect_timeout_secs),
                    args.allow_stub_dispatch_ready,
                    HOST_RPC_RECONNECT_INTERVAL,
                ));
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

    if let Some(handle) = gateway_guardian_watchdog {
        handle.abort();
    }
    if let Some(handle) = gateway_guardian_handle.lock().await.take() {
        handle.abort();
    }
    if let Some(handle) = gateway_guardian_publisher_handle {
        handle.abort();
    }

    if let Some(ctrl) = gateway_control.take() {
        ctrl.shutdown().await;
    }

    if let Some(handle) = host_rpc_reconnect_handle {
        handle.abort();
    }

    if let Some(handle) = mcp_handle {
        let state_close_url = handle.mcp_url.clone();
        handle.shutdown().await;
        tracing::info!(mcp_url = %state_close_url, "sidecar MCP listener stopped");
    }

    heartbeat_handle.abort();

    if let Err(err) = registry.deregister(&key) {
        tracing::warn!(error = %err, "FileRegistry deregister failed");
    }

    Ok(())
}

// -- tests --------------------------------------------------------------------

#[cfg(test)]
#[path = "sidecar_tests.rs"]
mod sidecar_tests;
