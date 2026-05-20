//! JSON shape persisted in gateway admin SQLite `audits.audit_json` rows.

use serde::{Deserialize, Serialize};

/// Stable audit envelope stored in SQLite (gateway admin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayAdminAuditPersistedJson {
    pub timestamp_ms: u64,
    pub request_id: String,
    pub method: Option<String>,
    pub instance_id: Option<String>,
    pub session_id: Option<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub agent_id: Option<String>,
    #[serde(default)]
    pub agent_name: Option<String>,
    #[serde(default)]
    pub agent_model: Option<String>,
    #[serde(default)]
    pub parent_request_id: Option<String>,
    pub action: String,
    pub dcc_type: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
}
