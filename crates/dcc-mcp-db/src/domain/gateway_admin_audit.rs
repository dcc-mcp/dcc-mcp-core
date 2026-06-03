//! JSON shape persisted in gateway admin SQLite `audits.audit_json` rows.

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Stable audit envelope stored in SQLite (gateway admin).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GatewayAdminAuditPersistedJson {
    pub timestamp_ms: u64,
    pub request_id: String,
    #[serde(default)]
    pub trace_id: Option<String>,
    #[serde(default)]
    pub span_id: Option<String>,
    #[serde(default)]
    pub parent_span_id: Option<String>,
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
    pub actor_id: Option<String>,
    #[serde(default)]
    pub actor_name: Option<String>,
    #[serde(default)]
    pub actor_email_hash: Option<String>,
    #[serde(default)]
    pub client_platform: Option<String>,
    #[serde(default)]
    pub client_os: Option<String>,
    #[serde(default)]
    pub client_host: Option<String>,
    #[serde(default)]
    pub auth_subject: Option<String>,
    #[serde(default)]
    pub source_ip: Option<String>,
    #[serde(default)]
    pub attribution_trust: Option<Value>,
    #[serde(default)]
    pub parent_request_id: Option<String>,
    pub action: String,
    pub dcc_type: Option<String>,
    pub success: bool,
    pub error: Option<String>,
    pub duration_ms: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub token_accounting: Option<Value>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub llm_usage: Option<Value>,
}
