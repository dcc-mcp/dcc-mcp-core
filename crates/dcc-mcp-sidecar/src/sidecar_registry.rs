//! FileRegistry row and metadata helpers for per-DCC sidecars.

use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_transport::discovery::file_registry::FileRegistry;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceKey, ServiceStatus};

use crate::sidecar::SidecarArgs;

/// FileRegistry `metadata` key used to tag sidecar rows.
///
/// Values are one of:
/// * `"per-dcc-sidecar"` - a sidecar child of a single DCC process
/// * `"gateway-sidecar"` - the machine-wide gateway sidecar (set elsewhere,
///   not by this subcommand)
pub const ROLE_METADATA_KEY: &str = "dcc_mcp_role";

/// Value stored in `metadata[ROLE_METADATA_KEY]` for per-DCC sidecars.
pub const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";

pub(crate) const FAILURE_REASON_METADATA_KEY: &str = "failure_reason";
pub(crate) const FAILURE_STAGE_METADATA_KEY: &str = "failure_stage";
pub(crate) const FAILURE_AT_UNIX_METADATA_KEY: &str = "failure_at_unix";
pub(crate) const HOST_RPC_URI_METADATA_KEY: &str = "host_rpc_uri";
pub(crate) const HOST_RPC_SCHEME_METADATA_KEY: &str = "host_rpc_scheme";
pub(crate) const DISPATCH_STATUS_METADATA_KEY: &str = "dispatch_status";
pub(crate) const DISPATCH_READY_AT_UNIX_METADATA_KEY: &str = "dispatch_ready_at_unix";
pub(crate) const GATEWAY_RUNTIME_MODE_METADATA_KEY: &str = "gateway_runtime_mode";
pub(crate) const GATEWAY_GUARDIAN_ENABLED_METADATA_KEY: &str = "gateway_guardian_enabled";
pub(crate) const DISPATCH_STATUS_BOOTING: &str = "booting";
pub(crate) const DISPATCH_STATUS_READY: &str = "ready";
pub(crate) const DISPATCH_STATUS_UNAVAILABLE: &str = "unavailable";

pub(crate) fn build_service_entry(args: &SidecarArgs) -> ServiceEntry {
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
    entry.metadata.insert(
        GATEWAY_RUNTIME_MODE_METADATA_KEY.to_string(),
        sidecar_gateway_runtime_mode(args).to_string(),
    );
    entry.metadata.insert(
        GATEWAY_GUARDIAN_ENABLED_METADATA_KEY.to_string(),
        sidecar_gateway_guardian_enabled(args).to_string(),
    );
    entry
        .metadata
        .insert("sidecar_pid".to_string(), std::process::id().to_string());

    entry
}

/// Re-write the FileRegistry row with the live MCP URL once the listener is
/// bound. The original `register()` call happens before the listener exists so
/// the row carries a placeholder `127.0.0.1:0` until this step runs.
///
/// Dispatch-ready sidecars become `Available`; diagnostic listeners keep
/// `Booting` plus `dispatch_status=unavailable` so gateway discovery can show
/// the URL for operators without routing calls through it.
pub(crate) fn republish_mcp_listener(
    registry: &Arc<FileRegistry>,
    key: &ServiceKey,
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
    // Deregister + register is atomic enough for our needs - the
    // FileRegistry only flushes after register() returns, so the
    // on-disk snapshot transitions in one step.
    registry.deregister(key)?;
    registry.register(entry)?;
    Ok(())
}

pub(crate) fn mark_sidecar_boot_failure(
    registry: &Arc<FileRegistry>,
    key: &ServiceKey,
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

pub(crate) fn mark_sidecar_dispatch_ready(
    registry: &Arc<FileRegistry>,
    key: &ServiceKey,
) -> anyhow::Result<()> {
    let Some(mut entry) = registry.get(key) else {
        anyhow::bail!("registry row vanished before sidecar dispatch-ready update")
    };
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
    registry.deregister(key)?;
    registry.register(entry)?;
    Ok(())
}

#[cfg(feature = "gateway-daemon")]
fn sidecar_gateway_runtime_mode(args: &SidecarArgs) -> &'static str {
    if args.gateway_port == 0 {
        "not_configured"
    } else if args.no_ensure_gateway {
        "failover_disabled_by_adapter"
    } else if args.legacy_gateway_election {
        "embedded-fallback"
    } else {
        "daemon-backed"
    }
}

#[cfg(not(feature = "gateway-daemon"))]
fn sidecar_gateway_runtime_mode(args: &SidecarArgs) -> &'static str {
    if args.gateway_port == 0 {
        "not_configured"
    } else if args.no_ensure_gateway {
        "failover_disabled_by_adapter"
    } else if args.legacy_gateway_election {
        "embedded-fallback"
    } else {
        "daemon-unavailable"
    }
}

#[cfg(feature = "gateway-daemon")]
fn sidecar_gateway_guardian_enabled(args: &SidecarArgs) -> bool {
    args.gateway_port > 0 && !args.no_ensure_gateway && !args.legacy_gateway_election
}

#[cfg(not(feature = "gateway-daemon"))]
fn sidecar_gateway_guardian_enabled(_args: &SidecarArgs) -> bool {
    false
}
