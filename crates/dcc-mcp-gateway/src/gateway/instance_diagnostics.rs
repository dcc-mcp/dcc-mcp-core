//! Per-backend diagnostics cached by the gateway (#1076).
//!
//! Populated by the health-check loop (readiness) and failed call paths
//! (last error). Surfaced on `gateway://instances`, `GET /v1/instances`,
//! and gateway-proxied error envelopes — not on successful search/call hits.

use std::collections::HashMap;
use std::time::{SystemTime, UNIX_EPOCH};

use dcc_mcp_skill_rest::ReadinessReport;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use uuid::Uuid;

use dcc_mcp_transport::discovery::types::ServiceEntry;

/// Summary of the last failed backend call for an instance.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct LastCallError {
    pub kind: String,
    pub message: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub at_unix_secs: Option<u64>,
}

/// Cached diagnostics for one DCC backend row.
#[derive(Debug, Clone, Default, Serialize, Deserialize, PartialEq, Eq)]
pub struct InstanceDiagnostics {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub readiness: Option<ReadinessReport>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub last_error: Option<LastCallError>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub probed_at_unix_secs: Option<u64>,
}

/// In-memory store keyed by `instance_id` (not persisted to `services.json`).
#[derive(Debug, Default)]
pub struct InstanceDiagnosticsStore {
    inner: RwLock<HashMap<Uuid, InstanceDiagnostics>>,
}

impl InstanceDiagnosticsStore {
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    pub fn record_readiness(&self, instance_id: Uuid, report: ReadinessReport) {
        let now = unix_now_secs();
        let mut map = self.inner.write();
        let entry = map.entry(instance_id).or_default();
        entry.readiness = Some(report);
        entry.probed_at_unix_secs = Some(now);
    }

    pub fn record_call_error(
        &self,
        instance_id: Uuid,
        kind: impl Into<String>,
        message: impl Into<String>,
    ) {
        let mut map = self.inner.write();
        let entry = map.entry(instance_id).or_default();
        entry.last_error = Some(LastCallError {
            kind: kind.into(),
            message: message.into(),
            at_unix_secs: Some(unix_now_secs()),
        });
    }

    #[must_use]
    pub fn get(&self, instance_id: &Uuid) -> Option<InstanceDiagnostics> {
        self.inner.read().get(instance_id).cloned()
    }

    /// Compact JSON for instance list rows and error envelopes.
    #[must_use]
    pub fn to_json_value(diag: &InstanceDiagnostics) -> Value {
        serde_json::to_value(diag).unwrap_or_else(|_| json!({}))
    }
}

/// Build the `backend` object attached to gateway call errors.
#[must_use]
pub fn backend_error_attachment(
    entry: &ServiceEntry,
    gateway_mcp_url: &str,
    diagnostics: Option<&InstanceDiagnostics>,
    backend_body: Option<&Value>,
) -> Value {
    let direct_mcp_url = format!("http://{}:{}/mcp", entry.host, entry.port);
    let mut backend = json!({
        "instance_id": entry.instance_id.to_string(),
        "display_id": entry.display_id(),
        "dcc_type": entry.dcc_type,
        "display_name": entry.display_name,
        "adapter_version": entry.adapter_version,
        "adapter_dcc": entry.adapter_dcc,
        "mcp_url": direct_mcp_url,
        "gateway_mcp_url": gateway_mcp_url,
    });
    if let Some(diag) = diagnostics {
        backend["diagnostics"] = InstanceDiagnosticsStore::to_json_value(diag);
    }
    if let Some(body) = backend_body {
        backend["error_body"] = body.clone();
    }
    backend
}

/// Try to extract a JSON `ServiceError` body from a REST forward failure string.
#[must_use]
pub fn parse_rest_error_json(err: &str) -> Option<Value> {
    let json_start = err.find('{')?;
    let slice = &err[json_start..];
    let value: Value = serde_json::from_str(slice).ok()?;
    if value.get("kind").is_some() {
        Some(value)
    } else {
        None
    }
}

fn unix_now_secs() -> u64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_rest_error_json_extracts_skill_rest_body() {
        let err = r#"http://127.0.0.1:8765: HTTP 409 Conflict: {"kind":"thread-affinity-violation","message":"x"}"#;
        let body = parse_rest_error_json(err).expect("json body");
        assert_eq!(body["kind"], "thread-affinity-violation");
    }

    #[test]
    fn store_records_readiness_and_last_error() {
        let store = InstanceDiagnosticsStore::new();
        let id = Uuid::new_v4();
        store.record_readiness(
            id,
            ReadinessReport {
                process: true,
                dispatcher: false,
                dcc: true,
            },
        );
        store.record_call_error(id, "thread-affinity-violation", "boom");
        let diag = store.get(&id).unwrap();
        assert!(!diag.readiness.as_ref().unwrap().dispatcher);
        assert_eq!(
            diag.last_error.as_ref().unwrap().kind,
            "thread-affinity-violation"
        );
    }
}
