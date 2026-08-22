//! Domain and embedded frontend boundary for the DCC-MCP gateway admin dashboard.
//!
//! This crate owns admin-facing audit, trace, caller-context, compact projection,
//! link, issue-report, statistics, analytics, governance, artifact, activity, task-outcome,
//! debug-bundle, postmortem, agent-trace packet, memory-summary, and traffic projections
//! independently of gateway routing state.
//! It also owns the Vite/npm build script and generated dashboard payload; the Node.js
//! toolchain only runs when `embed` is enabled.

#![forbid(unsafe_code)]

mod activity;
mod agent_trace;
mod analytics;
mod artifacts;
mod audit;
mod debug;
/// Admin trace and caller-attribution value types.
pub mod domain;
mod governance;
mod issue_report;
mod links;
mod memory;
mod projection;
mod stats;
mod tasks;
mod trace_log;
mod traffic;

pub use activity::{
    ActivityCorrelation, ActivityEvent, GatewayActivityInput, activity_payload,
    audit_activity_event, gateway_activity_event, gateway_activity_event_json,
    trace_activity_event,
};
pub use agent_trace::agent_trace_packet;
pub use analytics::{
    AnalyticsQuery, analytics_csv_export, analytics_heatmap_payload, analytics_jsonl_export,
    analytics_overview_payload, analytics_range_duration, analytics_timeseries_payload,
};
pub use artifacts::{ArtifactFilter, artifact_payload, artifact_refs};
pub use audit::{AdminAuditRecord, AuditLog};
pub use debug::debug_bundle_payload;
pub use domain::agent_context::{
    AgentContext, AgentContextTrust, INTERNAL_AUTH_SUBJECT_HEADER, INTERNAL_FORWARDED_FOR_HEADER,
    INTERNAL_SOURCE_IP_HEADER, TRUST_AUTH, TRUST_HEADER, TRUST_SELF_REPORTED, TRUST_SERVER_DERIVED,
    TRUST_TRUSTED_PROXY,
};
pub use domain::trace::{
    DispatchTrace, LlmUsage, MAX_AGENT_CONTEXT_LIST_ITEMS, MAX_AGENT_CONTEXT_METADATA_BYTES,
    MAX_AGENT_CONTEXT_STRING_BYTES, MAX_INPUT_BYTES, MAX_OUTPUT_BYTES, TOKEN_ESTIMATOR,
    TokenTelemetry, TraceContext, TraceContextHeader, TracePayload, TraceSpan, estimate_tokens,
    parse_traceparent,
};
pub use governance::{
    GovernanceCaptureDecision, GovernanceMiddlewareState, governance_payload, governance_stats,
};
pub use issue_report::{IssueReportMode, issue_report_filename, issue_report_json};
pub use links::AdminLinkBuilder;
pub use memory::memory_summary;
pub use projection::{
    compact_debug_bundle_payload, compact_trace_context_payload, compact_trace_detail_payload,
    compact_trace_list_payload,
};
pub use stats::{
    AttributionFacet, GatewayStats, LatencyStats, PayloadTokenUsageStats, StatsFilter, StatsRange,
    StatsStatus, TokenBreakdownEntry, TokenUsageStats, TopEntry, TraceStatsAggregator,
    compute_stats_filtered,
};
pub use tasks::{TaskArtifact, TaskRelated, TaskSnapshot, TaskValidation, task_payload};
pub use trace_log::TraceLog;
pub use traffic::{TrafficProjectionSnapshot, traffic_jsonl_export, traffic_payload};

/// The Vite-built React admin dashboard HTML page.
#[cfg(feature = "embed")]
pub const ADMIN_HTML: &str = include_str!("generated/index.html");

/// Minimal fallback for direct builds that do not request embedded assets.
#[cfg(not(feature = "embed"))]
pub const ADMIN_HTML: &str = r#"<!doctype html><html><head><meta charset="utf-8"><title>DCC-MCP Gateway Admin</title></head><body><h1>DCC-MCP Gateway Admin</h1><p>The embedded admin UI is not available in this build.</p></body></html>"#;

#[cfg(test)]
mod tests {
    use super::ADMIN_HTML;

    #[test]
    fn admin_html_is_a_complete_document() {
        assert!(ADMIN_HTML.starts_with("<!doctype html>"));
        assert!(ADMIN_HTML.contains("DCC-MCP"));
        assert!(ADMIN_HTML.trim_end().ends_with("</html>"));
    }
}
