//! Admin audit domain contracts.

use std::time::SystemTime;

use parking_lot::Mutex;

use crate::{AgentContextTrust, LlmUsage, TokenTelemetry};

/// Minimal audit record consumed by admin projections and persistence adapters.
#[derive(Debug, Clone)]
pub struct AdminAuditRecord {
    /// Wall-clock time when the call completed.
    pub timestamp: SystemTime,
    /// Stable request id used to correlate with traces.
    pub request_id: String,
    /// End-to-end trace id shared by related requests.
    pub trace_id: Option<String>,
    /// Root gateway span id for this request, if known.
    pub span_id: Option<String>,
    /// Incoming parent span id, if known.
    pub parent_span_id: Option<String>,
    /// JSON-RPC / MCP method name.
    pub method: Option<String>,
    /// Target backend instance id, if resolved.
    pub instance_id: Option<String>,
    /// Originating MCP session id, if any.
    pub session_id: Option<String>,
    /// Transport surface that produced the request (`mcp`, `rest`, ...).
    pub transport: Option<String>,
    /// Agent/caller id supplied for telemetry correlation.
    pub agent_id: Option<String>,
    /// Human-readable agent/caller name.
    pub agent_name: Option<String>,
    /// Model or runtime name supplied by the caller.
    pub agent_model: Option<String>,
    /// Human/service actor id supplied for telemetry filtering.
    pub actor_id: Option<String>,
    /// Human/service actor name supplied for telemetry filtering.
    pub actor_name: Option<String>,
    /// Hashed actor email or stable user handle. Never store raw email here.
    pub actor_email_hash: Option<String>,
    /// Client platform/runtime such as `cursor`, `claude-desktop`, or `custom-http`.
    pub client_platform: Option<String>,
    /// Client operating system label.
    pub client_os: Option<String>,
    /// Client host label.
    pub client_host: Option<String>,
    /// Authentication subject when provided by middleware/auth integration.
    pub auth_subject: Option<String>,
    /// Server-derived source IP after proxy trust policy.
    pub source_ip: Option<String>,
    /// Server-computed trust source labels for attribution fields.
    pub attribution_trust: Option<AgentContextTrust>,
    /// Parent request id for request-chain correlation.
    pub parent_request_id: Option<String>,
    /// Tool slug or MCP method name.
    pub action: String,
    /// DCC type of the target backend (e.g. `"maya"`).
    pub dcc_type: Option<String>,
    /// Whether the call succeeded (`true`) or returned an error (`false`).
    pub success: bool,
    /// Error preview when `success == false`; otherwise `None`.
    pub error: Option<String>,
    /// Wall-clock call duration in milliseconds.
    pub duration_ms: Option<u64>,
    /// Token accounting for the client-visible response, if available.
    pub token_accounting: Option<TokenTelemetry>,
    /// Optional upstream LLM billing token counts, when supplied.
    pub llm_usage: Option<LlmUsage>,
}

/// Bounded in-memory audit storage shared by gateway adapters.
pub type AuditLog = Mutex<Vec<AdminAuditRecord>>;
