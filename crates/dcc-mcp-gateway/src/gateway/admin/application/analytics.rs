//! Gateway adapters for admin analytics projections.

use std::time::SystemTime;

use axum::extract::{Query, State};
use axum::http::HeaderValue;
use axum::http::{StatusCode, header};
use axum::response::IntoResponse;

use super::super::state::{AdminAuditRecord, AdminState};

pub use dcc_mcp_gateway_admin::AnalyticsQuery;
use dcc_mcp_gateway_admin::{
    analytics_csv_export, analytics_heatmap_payload, analytics_jsonl_export,
    analytics_overview_payload, analytics_range_duration, analytics_timeseries_payload,
};

pub async fn handle_admin_analytics_overview(
    State(state): State<AdminState>,
    Query(query): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let now = SystemTime::now();
    let audits = fetch_audits(
        &state,
        now.checked_sub(analytics_range_duration(&query.range)),
    );
    (
        StatusCode::OK,
        axum::Json(analytics_overview_payload(&audits, &query.range, now)),
    )
        .into_response()
}

pub async fn handle_admin_analytics_timeseries(
    State(state): State<AdminState>,
    Query(query): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let audits = fetch_audits(
        &state,
        SystemTime::now().checked_sub(analytics_range_duration(&query.range)),
    );
    (
        StatusCode::OK,
        axum::Json(analytics_timeseries_payload(
            &audits,
            &query.range,
            &query.granularity,
        )),
    )
        .into_response()
}

pub async fn handle_admin_analytics_heatmap(
    State(state): State<AdminState>,
    Query(query): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let audits = fetch_audits(
        &state,
        SystemTime::now().checked_sub(analytics_range_duration(&query.range)),
    );
    (
        StatusCode::OK,
        axum::Json(analytics_heatmap_payload(&audits, &query.range)),
    )
        .into_response()
}

pub async fn handle_admin_analytics_export(
    State(state): State<AdminState>,
    Query(query): Query<AnalyticsQuery>,
) -> impl IntoResponse {
    let audits = fetch_audits(
        &state,
        SystemTime::now().checked_sub(analytics_range_duration(&query.range)),
    );
    let (body, content_type, extension) = if query.format == "csv" {
        (
            analytics_csv_export(&audits),
            "text/csv; charset=utf-8",
            "csv",
        )
    } else {
        (
            analytics_jsonl_export(&audits),
            "application/x-ndjson; charset=utf-8",
            "jsonl",
        )
    };
    let filename = format!("dcc-mcp-analytics-export-{}.{}", query.range, extension);
    let mut response = axum::response::Response::new(axum::body::Body::from(body));
    response
        .headers_mut()
        .insert(header::CONTENT_TYPE, HeaderValue::from_static(content_type));
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!("attachment; filename=\"{filename}\""))
            .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response
}

fn fetch_audits(state: &AdminState, cutoff: Option<SystemTime>) -> Vec<AdminAuditRecord> {
    if let Some(ref lane) = state.admin_sqlite_lane {
        lane.reader().list_audits_since(cutoff, 50_000)
    } else if let Some(ref log) = state.audit_log {
        log.lock()
            .iter()
            .filter(|audit| cutoff.is_none_or(|cutoff| audit.timestamp >= cutoff))
            .cloned()
            .collect()
    } else {
        Vec::new()
    }
}
