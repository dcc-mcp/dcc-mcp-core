//! Phase 4 — per-instance worker snapshots for the admin UI.
//!
//! Today we render workers from the data the gateway already has on hand
//! (registry-side `ServiceEntry`): `instance_id`, `dcc_type`, `pid`,
//! `mcp_url`, `status`, `display_name`, `registered_at`, `last_heartbeat`,
//! `version`, `adapter_version`.  This gives operators "which DCC is alive,
//! how long has it been alive, when did it last heartbeat" without having
//! to round-trip the backend.
//!
//! Runtime fields come from the shared instance-context collector: local
//! process/system metrics plus each backend's `/v1/context`.

use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_gateway_admin::{
    WorkerHealth, WorkerSnapshot, instance_server_binary_version, workers_payload,
};
use serde_json::Value;

use crate::gateway::http_registration::{MCP_URL_METADATA_KEY, entry_mcp_url};
use crate::gateway::instance_context::InstanceContext;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::types::{
    INSTANCE_TYPE_METADATA_KEY, ServiceEntry, ServiceStatus,
};

const DISPATCH_STATUS_METADATA_KEY: &str = "dispatch_status";
const DISPATCH_READY_AT_UNIX_METADATA_KEY: &str = "dispatch_ready_at_unix";
const HOST_RPC_URI_METADATA_KEY: &str = "host_rpc_uri";
const HOST_RPC_SCHEME_METADATA_KEY: &str = "host_rpc_scheme";
const GATEWAY_RUNTIME_MODE_METADATA_KEY: &str = "gateway_runtime_mode";
const GATEWAY_GUARDIAN_ENABLED_METADATA_KEY: &str = "gateway_guardian_enabled";
const GATEWAY_RECOVERY_DRIVER_METADATA_KEY: &str = "gateway_recovery_driver";
const REGISTRATION_REFRESH_MODE_METADATA_KEY: &str = "registration_refresh_mode";
const DISPATCH_STATUS_READY: &str = "ready";
const GATEWAY_RECOVERY_DRIVER_DAEMON_GUARDIAN: &str = "daemon_guardian";
const GATEWAY_RECOVERY_DRIVER_EMBEDDED_ELECTION: &str = "embedded_election";
const GATEWAY_RECOVERY_DRIVER_NONE: &str = "none";
const REGISTRATION_REFRESH_MODE_FILE_REGISTRY_HEARTBEAT: &str = "file_registry_heartbeat";
const ROLE_METADATA_KEY: &str = "dcc_mcp_role";
const ROLE_PER_DCC_SIDECAR: &str = "per-dcc-sidecar";

fn metadata_text(e: &ServiceEntry, key: &str) -> Option<String> {
    e.metadata
        .get(key)
        .map(String::as_str)
        .map(str::trim)
        .filter(|value| !value.is_empty())
        .map(ToOwned::to_owned)
}

fn metadata_bool(e: &ServiceEntry, key: &str) -> bool {
    e.metadata
        .get(key)
        .map(String::as_str)
        .map(str::trim)
        .is_some_and(|value| matches!(value.to_ascii_lowercase().as_str(), "true" | "1" | "yes"))
}

fn instance_type(e: &ServiceEntry) -> String {
    metadata_text(e, INSTANCE_TYPE_METADATA_KEY).unwrap_or_else(|| {
        if metadata_text(e, ROLE_METADATA_KEY).as_deref() == Some(ROLE_PER_DCC_SIDECAR)
            || e.adapter_version.is_some()
        {
            "gui".to_string()
        } else if instance_server_binary_version(e).is_some() {
            "standalone".to_string()
        } else {
            "unknown".to_string()
        }
    })
}

fn gateway_recovery_driver(
    e: &ServiceEntry,
    runtime_mode: Option<&str>,
    guardian_enabled: bool,
) -> String {
    metadata_text(e, GATEWAY_RECOVERY_DRIVER_METADATA_KEY).unwrap_or_else(|| {
        if guardian_enabled {
            GATEWAY_RECOVERY_DRIVER_DAEMON_GUARDIAN.to_string()
        } else if runtime_mode == Some("embedded-fallback") {
            GATEWAY_RECOVERY_DRIVER_EMBEDDED_ELECTION.to_string()
        } else {
            GATEWAY_RECOVERY_DRIVER_NONE.to_string()
        }
    })
}

/// Map gateway runtime state into the backend-neutral admin worker contract.
///
/// Infrastructure discovery and context collection remain owned by the gateway;
/// the admin crate owns the stable JSON projection.
fn entry_to_worker(
    e: &ServiceEntry,
    gs: &GatewayState,
    context: &InstanceContext,
) -> WorkerSnapshot {
    let stale = e.is_stale(gs.stale_timeout);
    let status = if stale {
        "stale".to_string()
    } else {
        e.status.to_string()
    };

    let registered_secs = e
        .registered_at
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());
    let last_heartbeat_secs = e
        .last_heartbeat
        .duration_since(UNIX_EPOCH)
        .ok()
        .map(|d| d.as_secs());

    // Uptime since the registry first observed this instance — best-effort
    // proxy for "how long has this DCC been alive".  If the system clock
    // moved backwards this can be 0; we surface 0 rather than a negative.
    let uptime_secs = SystemTime::now()
        .duration_since(e.registered_at)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let dispatch_status = metadata_text(e, DISPATCH_STATUS_METADATA_KEY);
    let dispatch_has_mcp_url = metadata_text(e, MCP_URL_METADATA_KEY).is_some();
    let dispatch_ready = dispatch_status.as_deref() == Some(DISPATCH_STATUS_READY)
        && dispatch_has_mcp_url
        && matches!(e.status, ServiceStatus::Available | ServiceStatus::Busy)
        && !stale;
    let gateway_runtime_mode = metadata_text(e, GATEWAY_RUNTIME_MODE_METADATA_KEY);
    let gateway_guardian_enabled = metadata_bool(e, GATEWAY_GUARDIAN_ENABLED_METADATA_KEY);
    let recovery_driver =
        gateway_recovery_driver(e, gateway_runtime_mode.as_deref(), gateway_guardian_enabled);
    let registration_refresh_mode = metadata_text(e, REGISTRATION_REFRESH_MODE_METADATA_KEY)
        .unwrap_or_else(|| REGISTRATION_REFRESH_MODE_FILE_REGISTRY_HEARTBEAT.to_string());
    let server_version = instance_server_binary_version(e).map(|(version, _)| version);

    WorkerSnapshot {
        instance_id: e.instance_id.to_string(),
        dcc_type: e.dcc_type.clone(),
        display_name: e.display_name.clone(),
        pid: e.pid,
        host_pid: e.host_pid,
        mcp_url: entry_mcp_url(e),
        host: e.host.clone(),
        port: e.port,
        status,
        stale,
        uptime_secs,
        registered_at_unix: registered_secs,
        last_heartbeat_unix: last_heartbeat_secs,
        version: e.version.clone(),
        server_version,
        adapter_version: e.adapter_version.clone(),
        instance_type: instance_type(e),
        adapter_dcc: e.adapter_dcc.clone(),
        scene: context.scene.clone(),
        documents: context.documents.clone(),
        loaded_skills: context.loaded_skills.clone(),
        loaded_skill_count: context.loaded_skill_count,
        action_count: context.action_count,
        performance: crate::gateway::instance_context::performance_json(context),
        failure_reason: e.metadata.get("failure_reason").cloned(),
        failure_stage: e.metadata.get("failure_stage").cloned(),
        dispatch_status,
        dispatch_ready,
        dispatch_ready_at_unix: metadata_text(e, DISPATCH_READY_AT_UNIX_METADATA_KEY),
        host_rpc_uri: metadata_text(e, HOST_RPC_URI_METADATA_KEY),
        host_rpc_scheme: metadata_text(e, HOST_RPC_SCHEME_METADATA_KEY),
        gateway_runtime_mode,
        gateway_guardian_enabled,
        gateway_recovery_driver: recovery_driver,
        registration_refresh_mode,
        metadata: serde_json::to_value(&e.metadata).unwrap_or_default(),
        cpu_percent: context.process.cpu_percent,
        memory_bytes: context.process.memory_bytes,
        virtual_memory_bytes: context.process.virtual_memory_bytes,
        backend_context_error: context.backend_context_error.clone(),
    }
}

/// Snapshot every alive instance into a Workers payload.
///
/// The admin Instances panel is an operator view of current DCC backends plus
/// still-alive diagnostics such as sidecars stuck in `Booting`. Stale/dead
/// registry rows are filtered, while `Booting` rows stay visible with their
/// structured failure metadata.
pub async fn build_workers_payload(gs: &GatewayState) -> Value {
    use dcc_mcp_transport::discovery::types::ServiceStatus;

    let live_instances = match gs.read_alive_instances_async().await {
        Ok((entries, _)) => entries,
        Err(_) => gs.all_instances_async().await,
    }
    .into_iter()
    .filter(|e| !e.is_stale(gs.stale_timeout))
    .collect::<Vec<_>>();

    let contexts = crate::gateway::instance_context::collect(gs, &live_instances).await;
    let workers = live_instances
        .iter()
        .map(|e| {
            let stale = e.is_stale(gs.stale_timeout);
            let health = if stale {
                WorkerHealth::Stale
            } else if matches!(e.status, ServiceStatus::Available | ServiceStatus::Busy) {
                WorkerHealth::Live
            } else {
                WorkerHealth::Unhealthy
            };
            let context = contexts.get(&e.instance_id).cloned().unwrap_or_default();
            (entry_to_worker(e, gs, &context), health)
        })
        .collect();
    workers_payload(workers)
}

#[cfg(test)]
mod tests {
    use super::*;
    use dcc_mcp_transport::discovery::types::SERVER_BINARY_VERSION_METADATA_KEY;

    #[test]
    fn instance_type_prefers_explicit_metadata() {
        let mut entry = ServiceEntry::new("houdini", "127.0.0.1", 18812);
        entry.metadata.insert(
            INSTANCE_TYPE_METADATA_KEY.to_string(),
            "standalone".to_string(),
        );

        assert_eq!(instance_type(&entry), "standalone");
    }

    #[test]
    fn instance_type_recovers_legacy_gui_and_standalone_rows() {
        let mut gui = ServiceEntry::new("maya", "127.0.0.1", 18813);
        gui.metadata.insert(
            ROLE_METADATA_KEY.to_string(),
            ROLE_PER_DCC_SIDECAR.to_string(),
        );
        let mut standalone = ServiceEntry::new("houdini", "127.0.0.1", 18814);
        standalone.metadata.insert(
            SERVER_BINARY_VERSION_METADATA_KEY.to_string(),
            "0.19.56".to_string(),
        );

        assert_eq!(instance_type(&gui), "gui");
        assert_eq!(instance_type(&standalone), "standalone");
    }
}
