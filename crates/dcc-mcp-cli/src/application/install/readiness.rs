//! Read-only, bounded readiness probe for a registered live adapter instance.
//!
//! A successful pip import proves the artifact is installed, not that the DCC
//! host can dispatch tools. This module reuses the exact single-probe logic of
//! `dcc-mcp-cli wait-ready` without starting a host or waiting on a loop.

use std::time::Duration;

use serde_json::Value;

use crate::application::gateway_ensure;
use crate::application::local_control;
use crate::application::local_instance;
use crate::domain::rest::WaitReadyRequest;

/// Missing readiness fields observed from a live adapter instance.
#[derive(Debug, PartialEq, Eq)]
pub(crate) struct ReadinessEvidence {
    pub(crate) missing: Vec<String>,
}

/// Probe a registered, dispatch-ready instance for the given DCC type.
///
/// Returns `None` when no routable instance exists or the host is unreachable;
/// callers keep the existing `LIVE_DCC_VERIFICATION_REQUIRED` verdict then.
pub(crate) fn probe_ready(dcc_type: &str) -> Option<ReadinessEvidence> {
    let dir = gateway_ensure::default_registry_dir().ok()?;
    // A registered row that does not advertise direct-control readiness cannot
    // be promoted, even if a stale readyz endpoint answers positively.
    let entry = local_instance::select_routable_entries(&dir, Some(dcc_type), None)
        .ok()?
        .into_iter()
        .next()?;
    let instance_id = entry.instance_id.to_string();

    // A dedicated thread keeps the synchronous install path working even when
    // the caller already runs inside a tokio runtime.
    let dcc_type = dcc_type.to_string();
    let result = std::thread::spawn(move || {
        let runtime = tokio::runtime::Builder::new_current_thread()
            .enable_all()
            .build()
            .ok()?;
        runtime
            .block_on(local_control::wait_ready_local(
                dir,
                WaitReadyRequest {
                    dcc_type: Some(dcc_type),
                    instance_id: Some(instance_id),
                    required: Vec::new(),
                    timeout: Duration::from_secs(3),
                    interval: Duration::from_millis(1),
                },
            ))
            .ok()
    })
    .join()
    .ok()
    .flatten()?;

    // Absent `missing` evidence is not an empty (complete) report: treat it as
    // all-required-missing so a malformed probe can never produce a false green.
    let missing = result
        .get("missing")
        .and_then(Value::as_array)
        .map(|values| {
            values
                .iter()
                .filter_map(Value::as_str)
                .map(str::to_string)
                .collect()
        })
        .unwrap_or_else(|| {
            ["process", "dcc", "skill_catalog", "dispatcher"]
                .iter()
                .map(|field| field.to_string())
                .collect()
        });
    Some(ReadinessEvidence { missing })
}
