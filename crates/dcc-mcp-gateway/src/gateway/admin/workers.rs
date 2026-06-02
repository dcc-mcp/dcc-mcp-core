//! Phase 4 — per-instance worker snapshots for the admin UI.
//!
//! Today we render workers from the data the gateway already has on hand
//! (registry-side `ServiceEntry`): `instance_id`, `dcc_type`, `pid`,
//! `mcp_url`, `status`, `display_name`, `registered_at`, `last_heartbeat`,
//! `version`, `adapter_version`.  This gives operators "which DCC is alive,
//! how long has it been alive, when did it last heartbeat" without having
//! to round-trip the backend.
//!
//! CPU / memory / RSS would require every backend to serve a per-process
//! diagnostic resource (the gateway's own `gateway://diagnostics/process`
//! is gateway-side only).  That is intentionally left for a follow-up so
//! Phase 4 can land without a cross-service contract change — see
//! issue #863 Phase 4 acceptance criteria.

use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::{Value, json};

use crate::gateway::http_registration::{MCP_URL_METADATA_KEY, entry_mcp_url};
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::types::{ServiceEntry, ServiceStatus};

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

/// Build a single Worker JSON record from a `ServiceEntry`.
///
/// Stable, low-allocation snapshot used by `GET /admin/api/workers`.
fn entry_to_worker_json(e: &ServiceEntry, gs: &GatewayState) -> Value {
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

    json!({
        "instance_id":          e.instance_id.to_string(),
        "dcc_type":             e.dcc_type,
        "display_name":         e.display_name,
        "pid":                  e.pid,
        "mcp_url":              entry_mcp_url(e),
        "host":                 e.host,
        "port":                 e.port,
        "status":               status,
        "stale":                stale,
        "uptime_secs":          uptime_secs,
        "registered_at_unix":   registered_secs,
        "last_heartbeat_unix":  last_heartbeat_secs,
        "version":              e.version,
        "adapter_version":      e.adapter_version,
        "adapter_dcc":          e.adapter_dcc,
        "scene":                e.scene,
        "failure_reason":       e.metadata.get("failure_reason").cloned(),
        "failure_stage":        e.metadata.get("failure_stage").cloned(),
        "dispatch_status":      dispatch_status,
        "dispatch_ready":       dispatch_ready,
        "dispatch_ready_at_unix": metadata_text(e, DISPATCH_READY_AT_UNIX_METADATA_KEY),
        "host_rpc_uri":         metadata_text(e, HOST_RPC_URI_METADATA_KEY),
        "host_rpc_scheme":      metadata_text(e, HOST_RPC_SCHEME_METADATA_KEY),
        "gateway_runtime_mode":  gateway_runtime_mode,
        "gateway_guardian_enabled": gateway_guardian_enabled,
        "gateway_recovery_driver": recovery_driver,
        "registration_refresh_mode": registration_refresh_mode,
        "metadata":             e.metadata,
        // CPU / memory not yet available — see module docs.
        "cpu_percent":          Value::Null,
        "memory_bytes":         Value::Null,
    })
}

/// Snapshot every alive instance into a Workers payload.
///
/// The admin Instances panel is an operator view of current DCC backends plus
/// still-alive diagnostics such as sidecars stuck in `Booting`. Stale/dead
/// registry rows are filtered, while `Booting` rows stay visible with their
/// structured failure metadata.
pub async fn build_workers_payload(gs: &GatewayState) -> Value {
    use dcc_mcp_transport::discovery::types::ServiceStatus;

    let reg = gs.registry.read().await;
    let live_instances = gs
        .read_alive_instances(&reg)
        .map(|(entries, _)| entries)
        .unwrap_or_else(|_| gs.all_instances(&reg))
        .into_iter()
        .filter(|e| !e.is_stale(gs.stale_timeout))
        .collect::<Vec<_>>();

    let mut live = 0usize;
    let mut stale_count = 0usize;
    let mut unhealthy = 0usize;
    let workers: Vec<Value> = live_instances
        .iter()
        .map(|e| {
            let stale = e.is_stale(gs.stale_timeout);
            if stale {
                stale_count += 1;
            } else if matches!(e.status, ServiceStatus::Available | ServiceStatus::Busy) {
                live += 1;
            } else {
                unhealthy += 1;
            }
            entry_to_worker_json(e, gs)
        })
        .collect();

    json!({
        "total": workers.len(),
        "summary": {
            "live":      live,
            "stale":     stale_count,
            "unhealthy": unhealthy,
        },
        "workers": workers,
    })
}
