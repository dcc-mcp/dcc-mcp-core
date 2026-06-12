//! Admin UI axum router — registered only when the `admin` feature is enabled.

use axum::{Router, routing};

use super::agent_trace::handle_v1_debug_agent_trace_packet;
use super::analytics::{
    handle_admin_analytics_export, handle_admin_analytics_heatmap, handle_admin_analytics_overview,
    handle_admin_analytics_timeseries,
};
use super::general::{
    handle_admin_activity, handle_admin_governance, handle_admin_traffic,
    handle_admin_traffic_export, handle_admin_ui,
};
use super::handlers::{
    handle_admin_calls, handle_admin_debug_bundle, handle_admin_deregistered, handle_admin_health,
    handle_admin_instance_update, handle_admin_instances, handle_admin_issue_report,
    handle_admin_logs, handle_admin_search_telemetry, handle_admin_skill_detail,
    handle_admin_skills, handle_admin_stats, handle_admin_tasks, handle_admin_tools,
    handle_admin_trace_detail, handle_admin_traces, handle_admin_workers, handle_admin_workflows,
    handle_v1_debug_trace_lookup,
};
use super::integrations::{handle_admin_integration_update, handle_admin_integrations};
use super::marketplace::{
    handle_marketplace_add_source, handle_marketplace_catalog, handle_marketplace_install,
    handle_marketplace_installed, handle_marketplace_outdated, handle_marketplace_sources,
    handle_marketplace_uninstall, handle_marketplace_update,
};
use super::skill_paths::{
    handle_admin_skill_path_add, handle_admin_skill_path_delete, handle_admin_skill_paths,
};
use super::state::AdminState;

/// Build the admin sub-router.
///
/// Mount this under `admin_path` (default `"/admin"`) on the main gateway
/// router when `admin_enabled = true`.
///
/// Routes provided:
/// - `GET  /`              → HTML dashboard
/// - `GET  /api/instances` → JSON instance list
/// - `GET  /api/tools`     → JSON tool list
/// - `GET  /api/skills`    → JSON skill list
/// - `GET  /api/skill-detail?name=...` → one skill's detailed markdown/info
/// - `GET  /api/calls`              → JSON recent calls
/// - `GET  /api/traces`             → JSON recent dispatch traces (Phase 2)
/// - `GET  /api/traces/{request_id}` → full trace waterfall for one call
/// - `GET  /api/traffic`            → capture state + retained safe traffic metadata
/// - `GET  /api/traffic/export`     → retained safe traffic metadata as JSONL
/// - `GET  /api/issue-report/{request_id}` → downloadable JSON issue report
/// - `GET  /api/workflows`          → agent/session workflow projection
/// - `GET  /api/stats?range=1h|24h|7d` → aggregated call statistics (Phase 3)
/// - `GET  /api/governance`        → traffic policy/capture/redaction/pressure state
/// - `GET  /api/workers`            → per-instance worker cards (Phase 4)
/// - `GET  /api/deregistered`       → recently auto-deregistered rows
/// - `GET  /api/logs`               → JSON event log
/// - `GET  /api/health`             → JSON health summary
pub fn build_admin_router(state: AdminState) -> Router {
    Router::new()
        .route("/", routing::get(handle_admin_ui))
        .route("/api/activity", routing::get(handle_admin_activity))
        .route("/api/instances", routing::get(handle_admin_instances))
        .route(
            "/api/instances/{instance_id}/update",
            routing::post(handle_admin_instance_update),
        )
        .route("/api/tools", routing::get(handle_admin_tools))
        .route("/api/skills", routing::get(handle_admin_skills))
        .route("/api/skill-detail", routing::get(handle_admin_skill_detail))
        .route("/api/calls", routing::get(handle_admin_calls))
        .route("/api/traces", routing::get(handle_admin_traces))
        .route("/api/traffic", routing::get(handle_admin_traffic))
        .route(
            "/api/traffic/export",
            routing::get(handle_admin_traffic_export),
        )
        .route(
            "/api/traces/{request_id}",
            routing::get(handle_admin_trace_detail),
        )
        .route("/api/tasks", routing::get(handle_admin_tasks))
        .route("/api/workflows", routing::get(handle_admin_workflows))
        .route(
            "/api/debug-bundle/{request_id}",
            routing::get(handle_admin_debug_bundle),
        )
        .route(
            "/api/issue-report/{request_id}",
            routing::get(handle_admin_issue_report),
        )
        .route("/api/skill-paths", routing::get(handle_admin_skill_paths))
        .route(
            "/api/skill-paths",
            routing::post(handle_admin_skill_path_add),
        )
        .route(
            "/api/skill-paths/{id}",
            routing::delete(handle_admin_skill_path_delete),
        )
        .route("/api/logs", routing::get(handle_admin_logs))
        .route("/api/deregistered", routing::get(handle_admin_deregistered))
        .route("/api/stats", routing::get(handle_admin_stats))
        .route(
            "/api/analytics/overview",
            routing::get(handle_admin_analytics_overview),
        )
        .route(
            "/api/analytics/timeseries",
            routing::get(handle_admin_analytics_timeseries),
        )
        .route(
            "/api/analytics/heatmap",
            routing::get(handle_admin_analytics_heatmap),
        )
        .route(
            "/api/analytics/export",
            routing::get(handle_admin_analytics_export),
        )
        .route("/api/governance", routing::get(handle_admin_governance))
        .route(
            "/api/search-telemetry",
            routing::get(handle_admin_search_telemetry),
        )
        .route("/api/workers", routing::get(handle_admin_workers))
        .route("/api/health", routing::get(handle_admin_health))
        .route(
            "/api/integrations",
            routing::get(handle_admin_integrations).put(handle_admin_integration_update),
        )
        .route(
            "/api/marketplace/catalog",
            routing::get(handle_marketplace_catalog),
        )
        .route(
            "/api/marketplace/installed",
            routing::get(handle_marketplace_installed),
        )
        .route(
            "/api/marketplace/install",
            routing::post(handle_marketplace_install),
        )
        .route(
            "/api/marketplace/uninstall",
            routing::post(handle_marketplace_uninstall),
        )
        .route(
            "/api/marketplace/sources",
            routing::get(handle_marketplace_sources),
        )
        .route(
            "/api/marketplace/sources",
            routing::post(handle_marketplace_add_source),
        )
        .route(
            "/api/marketplace/outdated",
            routing::get(handle_marketplace_outdated),
        )
        .route(
            "/api/marketplace/update",
            routing::post(handle_marketplace_update),
        )
        .with_state(state)
}

/// Build stable `/v1/debug/*` routes backed by the admin trace store.
pub fn build_v1_debug_router(state: AdminState) -> Router {
    Router::new()
        .route("/v1/debug/instances", routing::get(handle_admin_instances))
        .route("/v1/debug/activity", routing::get(handle_admin_activity))
        .route("/v1/debug/calls", routing::get(handle_admin_calls))
        .route("/v1/debug/traces", routing::get(handle_admin_traces))
        .route("/v1/debug/traffic", routing::get(handle_admin_traffic))
        .route(
            "/v1/debug/traffic/export",
            routing::get(handle_admin_traffic_export),
        )
        .route(
            "/v1/debug/traces/{request_id}",
            routing::get(handle_admin_trace_detail),
        )
        .route(
            "/v1/debug/trace-context/{lookup_id}",
            routing::get(handle_v1_debug_trace_lookup),
        )
        .route(
            "/v1/debug/agent-traces/{lookup_id}",
            routing::get(handle_v1_debug_agent_trace_packet),
        )
        .route("/v1/debug/tasks", routing::get(handle_admin_tasks))
        .route("/v1/debug/workflows", routing::get(handle_admin_workflows))
        .route(
            "/v1/debug/issue-reports/{request_id}",
            routing::get(handle_admin_issue_report),
        )
        .route(
            "/v1/debug/bundles/{request_id}",
            routing::get(handle_admin_debug_bundle),
        )
        .route("/v1/debug/logs", routing::get(handle_admin_logs))
        .route(
            "/v1/debug/deregistered",
            routing::get(handle_admin_deregistered),
        )
        .route("/v1/debug/stats", routing::get(handle_admin_stats))
        .route(
            "/v1/debug/analytics/overview",
            routing::get(handle_admin_analytics_overview),
        )
        .route(
            "/v1/debug/analytics/timeseries",
            routing::get(handle_admin_analytics_timeseries),
        )
        .route(
            "/v1/debug/analytics/heatmap",
            routing::get(handle_admin_analytics_heatmap),
        )
        .route(
            "/v1/debug/analytics/export",
            routing::get(handle_admin_analytics_export),
        )
        .route(
            "/v1/debug/governance",
            routing::get(handle_admin_governance),
        )
        .route(
            "/v1/debug/search-telemetry",
            routing::get(handle_admin_search_telemetry),
        )
        .route(
            "/v1/debug/integrations",
            routing::get(handle_admin_integrations),
        )
        .route("/v1/debug/health", routing::get(handle_admin_health))
        .with_state(state)
}
