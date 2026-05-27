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

use crate::gateway::http_registration::entry_mcp_url;
use crate::gateway::state::GatewayState;
use dcc_mcp_transport::discovery::types::ServiceEntry;

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
