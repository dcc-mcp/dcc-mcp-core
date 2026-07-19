//! Structured tool-call event model for the observability system (PIP-2751).
//!
//! Replaces opaque audit JSON with a structured schema that enables:
//! - Session → request → batch child → tool/skill trace chains
//! - Time-range and session-based queries
//! - Metric aggregation by tool, skill, DCC type, and result

use serde::{Deserialize, Serialize};

/// A single tool-call event with full attribution.
///
/// Stored in the `tool_calls` table of the gateway admin SQLite database.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallEvent {
    /// Unique request identifier (JSON-RPC `id`).
    pub request_id: String,

    /// Session this call belongs to.
    pub session_id: String,

    /// Parent request ID — for batch sub-call attribution.
    /// When `call_batch` dispatches individual tool calls, each child
    /// records the batch's `request_id` here. Non-batched calls have `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_request_id: Option<String>,

    /// Batch group identifier — all sub-calls from the same `call_batch`
    /// share the same `batch_id`. Non-batched calls have `None`.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub batch_id: Option<String>,

    /// Fully-qualified tool name (e.g. `"create_sphere"`).
    pub tool_name: String,

    /// Skill name if the tool belongs to a loaded skill.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub skill_name: Option<String>,

    /// DCC application type (e.g. `"maya"`, `"blender"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub dcc_type: Option<String>,

    /// DCC instance identifier.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub instance_id: Option<String>,

    /// Agent identifier that initiated the call.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub agent_id: Option<String>,

    /// Transport protocol used (e.g. `"rest"`, `"mcp"`, `"cli"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub transport: Option<String>,

    /// Whether the tool call was initiated via gateway or CLI direct.
    /// `true` = gateway, `false` = CLI direct.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub via_gateway: Option<bool>,

    /// Millisecond timestamp when execution started.
    pub started_at_ms: i64,

    /// Execution duration in milliseconds.
    pub duration_ms: i64,

    /// Whether the tool call succeeded.
    pub success: bool,

    /// Error message if the call failed.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_message: Option<String>,

    /// Error category (small fixed vocabulary for grouping).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error_kind: Option<String>,

    /// MCP method used (e.g. `"tools/call"`, `"resources/read"`).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub mcp_method: Option<String>,

    /// OpenTelemetry trace ID (for distributed tracing correlation).
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,

    /// OpenTelemetry span ID.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub span_id: Option<String>,
}

impl ToolCallEvent {
    /// Create a new tool-call event with the minimum required fields.
    #[must_use]
    pub fn new(
        request_id: String,
        session_id: String,
        tool_name: String,
        started_at_ms: i64,
        duration_ms: i64,
        success: bool,
    ) -> Self {
        Self {
            request_id,
            session_id,
            parent_request_id: None,
            batch_id: None,
            tool_name,
            skill_name: None,
            dcc_type: None,
            instance_id: None,
            agent_id: None,
            transport: None,
            via_gateway: None,
            started_at_ms,
            duration_ms,
            success,
            error_message: None,
            error_kind: None,
            mcp_method: None,
            trace_id: None,
            span_id: None,
        }
    }

    /// Mark this event as a batch child with parent and batch ID.
    pub fn with_batch_parent(mut self, parent_request_id: String, batch_id: String) -> Self {
        self.parent_request_id = Some(parent_request_id);
        self.batch_id = Some(batch_id);
        self
    }

    /// Set DCC context.
    pub fn with_dcc_context(mut self, dcc_type: String, instance_id: Option<String>) -> Self {
        self.dcc_type = Some(dcc_type);
        self.instance_id = instance_id;
        self
    }

    /// Set agent context.
    pub fn with_agent(mut self, agent_id: String) -> Self {
        self.agent_id = Some(agent_id);
        self
    }

    /// Set transport context.
    pub fn with_transport(mut self, transport: String, via_gateway: bool) -> Self {
        self.transport = Some(transport);
        self.via_gateway = Some(via_gateway);
        self
    }

    /// Set error details.
    pub fn with_error(mut self, message: String, kind: Option<String>) -> Self {
        self.error_message = Some(message);
        self.error_kind = kind;
        self
    }

    /// Set skill name.
    pub fn with_skill(mut self, skill_name: String) -> Self {
        self.skill_name = Some(skill_name);
        self
    }

    /// Set MCP method.
    pub fn with_mcp_method(mut self, method: String) -> Self {
        self.mcp_method = Some(method);
        self
    }

    /// Set tracing IDs.
    pub fn with_trace(mut self, trace_id: String, span_id: String) -> Self {
        self.trace_id = Some(trace_id);
        self.span_id = Some(span_id);
        self
    }
}

/// Aggregated tool-call statistics for a time range.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ToolCallStats {
    /// Total number of tool calls.
    pub total_calls: u64,
    /// Number of successful calls.
    pub success_count: u64,
    /// Number of failed calls.
    pub failure_count: u64,
    /// Success rate (0.0 to 1.0).
    pub success_rate: f64,
    /// Average duration in milliseconds.
    pub avg_duration_ms: f64,
    /// P50 duration in milliseconds.
    pub p50_duration_ms: f64,
    /// P95 duration in milliseconds.
    pub p95_duration_ms: f64,
    /// P99 duration in milliseconds.
    pub p99_duration_ms: f64,
}

/// Aggregated session statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SessionStats {
    /// Total sessions in the time range.
    pub total_sessions: u64,
    /// Currently active sessions.
    pub active_sessions: u64,
    /// Sessions that ended normally.
    pub ended_normally: u64,
    /// Sessions that ended abnormally (crashed, disconnected, etc.).
    pub ended_abnormally: u64,
    /// Average session duration in milliseconds.
    pub avg_duration_ms: f64,
    /// Total tool calls across all sessions.
    pub total_tool_calls: u64,
    /// Total errors across all sessions.
    pub total_errors: u64,
}

/// Coverage statistics for observability completeness.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CoverageStats {
    /// Total observed requests (gateway-proxied).
    pub observed_requests: u64,
    /// Total unobserved requests (CLI direct, not proxied).
    pub unobserved_requests: u64,
    /// Coverage ratio (observed / (observed + unobserved)).
    /// Returns 0.0 when there are no requests.
    pub coverage_ratio: f64,
}

/// Crash statistics for stability tracking.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CrashStats {
    /// Total crashes by type.
    pub total_crashes: u64,
    /// Host (DCC process) crashes.
    pub host_crashes: u64,
    /// GPU-related crashes.
    pub gpu_crashes: u64,
    /// Reconnect count.
    pub reconnects: u64,
    /// Recovery count (successful reconnects).
    pub recoveries: u64,
}

/// Capability funnel statistics (search → load → call → success).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct FunnelStats {
    /// Total skill searches.
    pub searches_total: u64,
    /// Searches that returned zero results.
    pub searches_zero_results: u64,
    /// Skills loaded.
    pub skills_loaded: u64,
    /// Skills called.
    pub skills_called: u64,
    /// Skills that succeeded.
    pub skills_succeeded: u64,
    /// Script fallback calls.
    pub script_fallbacks: u64,
    /// UI control fallback calls.
    pub ui_control_fallbacks: u64,
}

/// Artifact and result quality statistics.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ArtifactStats {
    /// Total artifacts generated.
    pub artifacts_generated: u64,
    /// Artifacts saved.
    pub artifacts_saved: u64,
    /// Artifacts exported.
    pub artifacts_exported: u64,
    /// Artifacts validated successfully.
    pub artifacts_validated_ok: u64,
    /// Artifacts that failed validation.
    pub artifacts_validated_fail: u64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tool_call_event_basic() {
        let event = ToolCallEvent::new(
            "req-1".into(),
            "sess-1".into(),
            "create_sphere".into(),
            1_700_000_000_000,
            42,
            true,
        );
        assert_eq!(event.request_id, "req-1");
        assert_eq!(event.session_id, "sess-1");
        assert_eq!(event.tool_name, "create_sphere");
        assert!(event.parent_request_id.is_none());
        assert!(event.batch_id.is_none());
    }

    #[test]
    fn tool_call_event_batch_child() {
        let event = ToolCallEvent::new(
            "child-1".into(),
            "sess-1".into(),
            "get_scene_objects".into(),
            1_700_000_000_000,
            15,
            true,
        )
        .with_batch_parent("batch-req".into(), "batch-grp-1".into());
        assert_eq!(event.parent_request_id.as_deref(), Some("batch-req"));
        assert_eq!(event.batch_id.as_deref(), Some("batch-grp-1"));
    }

    #[test]
    fn tool_call_event_full() {
        let event = ToolCallEvent::new(
            "req-1".into(),
            "sess-1".into(),
            "render_frame".into(),
            1_700_000_000_000,
            5000,
            false,
        )
        .with_dcc_context("maya".into(), Some("inst-42".into()))
        .with_agent("agent-7".into())
        .with_transport("mcp".into(), true)
        .with_skill("rendering".into())
        .with_mcp_method("tools/call".into())
        .with_error("CUDA out of memory".into(), Some("gpu".into()))
        .with_trace("trace-abc".into(), "span-def".into());

        let json = serde_json::to_string(&event).unwrap();
        let back: ToolCallEvent = serde_json::from_str(&json).unwrap();
        assert_eq!(back.request_id, "req-1");
        assert!(!back.success);
        assert_eq!(back.error_kind.as_deref(), Some("gpu"));
        assert_eq!(back.dcc_type.as_deref(), Some("maya"));
        assert_eq!(back.via_gateway, Some(true));
        assert_eq!(back.trace_id.as_deref(), Some("trace-abc"));
    }

    #[test]
    fn coverage_ratio_calculation() {
        let stats = CoverageStats {
            observed_requests: 100,
            unobserved_requests: 50,
            coverage_ratio: 100.0 / 150.0,
        };
        assert!((stats.coverage_ratio - 0.6666).abs() < 0.01);
    }

    #[test]
    fn empty_coverage_is_zero() {
        let stats = CoverageStats {
            observed_requests: 0,
            unobserved_requests: 0,
            coverage_ratio: 0.0,
        };
        assert_eq!(stats.coverage_ratio, 0.0);
    }
}
