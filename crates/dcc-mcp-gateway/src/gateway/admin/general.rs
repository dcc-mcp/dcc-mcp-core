use axum::Json;
use axum::extract::{OriginalUri, Query, State};
use axum::http::{HeaderMap, HeaderValue, StatusCode, header};
use axum::response::IntoResponse;
use serde_json::json;

use super::debug_response::DebugListQuery;
use super::html::ADMIN_HTML;
use super::links::AdminLinkBuilder;
use super::state::AdminState;

fn traffic_export_filename() -> &'static str {
    "dcc-mcp-traffic-capture.jsonl"
}

/// `GET /admin` — serve the inline HTML dashboard.
pub async fn handle_admin_ui() -> impl IntoResponse {
    let mut resp = axum::response::Html(ADMIN_HTML).into_response();
    resp.headers_mut().insert(
        header::CACHE_CONTROL,
        HeaderValue::from_static("no-cache, no-store"),
    );
    resp
}

/// `GET /admin/api/activity` — unified operator / agent activity timeline.
pub async fn handle_admin_activity(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::activity::build_activity_payload(&s, limit).await)
}

/// `GET /admin/api/governance` — effective traffic governance policy and decisions.
pub async fn handle_admin_governance(
    State(s): State<AdminState>,
    axum::extract::Query(params): axum::extract::Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::governance::build_governance_payload(&s, limit).await)
}

/// `GET /admin/api/traffic?limit=200` — retained live traffic frames.
pub async fn handle_admin_traffic(
    State(s): State<AdminState>,
    headers: HeaderMap,
    OriginalUri(uri): OriginalUri,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let links = AdminLinkBuilder::from_request(&headers, &uri);
    let limit = params.limit(200, 1_000);
    Json(crate::gateway::admin::traffic::build_traffic_payload(
        &s.gateway.traffic_capture,
        limit,
        json!({
            "admin_traffic_url": links.panel_url("traffic"),
            "traffic_api_url": links.api_url("/traffic"),
            "traffic_export_jsonl_url": links.api_url("/traffic/export"),
        }),
    ))
}

/// `GET /admin/api/traffic/export?limit=1000` — retained live frames as JSONL.
pub async fn handle_admin_traffic_export(
    State(s): State<AdminState>,
    Query(params): Query<DebugListQuery>,
) -> impl IntoResponse {
    let limit = params.limit(1_000, 10_000);
    let body = crate::gateway::admin::traffic::build_traffic_export_body(
        &s.gateway.traffic_capture,
        limit,
    );
    let mut response = (StatusCode::OK, body).into_response();
    response.headers_mut().insert(
        header::CONTENT_TYPE,
        HeaderValue::from_static("application/x-ndjson; charset=utf-8"),
    );
    response.headers_mut().insert(
        header::CONTENT_DISPOSITION,
        HeaderValue::from_str(&format!(
            "attachment; filename=\"{}\"",
            traffic_export_filename()
        ))
        .unwrap_or_else(|_| HeaderValue::from_static("attachment")),
    );
    response
}
