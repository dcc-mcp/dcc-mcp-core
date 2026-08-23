use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::memory_summary;
use serde::Deserialize;
use serde_json::{Value, json};

use super::super::state::AdminState;

#[derive(Debug, Deserialize)]
pub struct MemoryListQuery {
    pub limit: Option<usize>,
    pub layer: Option<String>,
    #[serde(alias = "dcc")]
    pub dcc_name: Option<String>,
    pub session_id: Option<String>,
    pub key_prefix: Option<String>,
}

#[derive(Debug, Deserialize)]
pub struct MemoryForgetBody {
    pub id: Option<i64>,
    pub layer: Option<String>,
    #[serde(alias = "dcc")]
    pub dcc_name: Option<String>,
    pub session_id: Option<String>,
    pub key_prefix: Option<String>,
}

pub async fn handle_admin_memory(
    State(s): State<AdminState>,
    Query(query): Query<MemoryListQuery>,
) -> impl IntoResponse {
    let Some(ref lane) = s.admin_sqlite_lane else {
        return Json(json!({
            "enabled": false,
            "memory": [],
            "summary": memory_summary(&[]),
            "error": "admin sqlite lane disabled",
        }))
        .into_response();
    };
    let limit = query.limit.unwrap_or(200).clamp(1, 1_000);
    let layer = clean(query.layer);
    let dcc_name = clean(query.dcc_name);
    let session_id = clean(query.session_id);
    let key_prefix = clean(query.key_prefix);
    let rows = lane.reader().list_agent_memory(
        layer.as_deref(),
        dcc_name.as_deref(),
        session_id.as_deref(),
        key_prefix.as_deref(),
        limit,
    );
    Json(json!({
        "enabled": true,
        "memory": rows,
        "summary": memory_summary(&rows),
    }))
    .into_response()
}

pub async fn handle_admin_memory_forget(
    State(s): State<AdminState>,
    Json(body): Json<MemoryForgetBody>,
) -> impl IntoResponse {
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    let layer = clean(body.layer);
    let dcc_name = clean(body.dcc_name);
    let session_id = clean(body.session_id);
    let key_prefix = clean(body.key_prefix);
    if body.id.is_none() && dcc_name.is_none() && session_id.is_none() && key_prefix.is_none() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "memory forget requires id, dcc_name, session_id, or key_prefix" })),
        )
            .into_response();
    }
    if lane.try_delete_agent_memory(body.id, layer, dcc_name, session_id, key_prefix) {
        if let Some(id) = body.id {
            wait_until_agent_memory_id_removed(lane, id).await;
        }
        (StatusCode::OK, Json(json!({ "ok": true }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}

async fn wait_until_agent_memory_id_removed(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    id: i64,
) {
    for _ in 0..80 {
        if !lane
            .reader()
            .list_agent_memory(None, None, None, None, 1_000)
            .iter()
            .any(|row| row.get("id").and_then(Value::as_i64) == Some(id))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(memory_id = id, "agent memory id not removed after 2 s poll");
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
