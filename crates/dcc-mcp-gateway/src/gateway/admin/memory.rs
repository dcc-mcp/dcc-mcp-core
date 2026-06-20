use std::collections::BTreeMap;
use std::time::Duration;

use axum::Json;
use axum::extract::{Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::{Value, json};

use super::state::AdminState;

#[derive(Debug, Deserialize)]
pub struct MemoryListQuery {
    pub limit: Option<usize>,
    pub layer: Option<String>,
    #[serde(alias = "dcc")]
    pub dcc_name: Option<String>,
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
            "summary": build_memory_summary(&[]),
            "error": "admin sqlite lane disabled",
        }))
        .into_response();
    };
    let limit = query.limit.unwrap_or(200).clamp(1, 1_000);
    let layer = clean(query.layer);
    let dcc_name = clean(query.dcc_name);
    let key_prefix = clean(query.key_prefix);
    let rows = lane.reader().list_agent_memory(
        layer.as_deref(),
        dcc_name.as_deref(),
        key_prefix.as_deref(),
        limit,
    );
    Json(json!({
        "enabled": true,
        "memory": rows,
        "summary": build_memory_summary(&rows),
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
            .list_agent_memory(None, None, None, 1_000)
            .iter()
            .any(|row| row.get("id").and_then(Value::as_i64) == Some(id))
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(memory_id = id, "agent memory id not removed after 2 s poll");
}

fn build_memory_summary(rows: &[Value]) -> Value {
    let mut by_dcc = BTreeMap::<String, usize>::new();
    let mut positive = 0usize;
    let mut negative = 0usize;
    let mut ok_count = 0u64;
    let mut fail_count = 0u64;

    for row in rows {
        let dcc = row
            .get("dcc_name")
            .and_then(Value::as_str)
            .unwrap_or("unknown")
            .to_string();
        *by_dcc.entry(dcc).or_default() += 1;
        let score = row.get("score").and_then(Value::as_f64).unwrap_or(0.0);
        if score > 0.0 {
            positive += 1;
        } else if score < 0.0 {
            negative += 1;
        }
        let (ok, fail) = outcome_counts(row.get("payload").unwrap_or(&Value::Null));
        ok_count += ok;
        fail_count += fail;
    }
    let hit_rate_pct = if ok_count + fail_count > 0 {
        Some((ok_count as f64 / (ok_count + fail_count) as f64) * 100.0)
    } else {
        None
    };
    json!({
        "total": rows.len(),
        "by_dcc": by_dcc,
        "positive": positive,
        "negative": negative,
        "ok_count": ok_count,
        "fail_count": fail_count,
        "hit_rate_pct": hit_rate_pct,
    })
}

fn outcome_counts(payload: &Value) -> (u64, u64) {
    let ok = payload.get("ok_count").and_then(Value::as_u64);
    let fail = payload.get("fail_count").and_then(Value::as_u64);
    if ok.is_some() || fail.is_some() {
        return (ok.unwrap_or(0), fail.unwrap_or(0));
    }
    match payload.get("ok").and_then(Value::as_bool) {
        Some(true) => (1, 0),
        Some(false) => (0, 1),
        None => (0, 0),
    }
}

fn clean(value: Option<String>) -> Option<String> {
    value
        .map(|value| value.trim().to_string())
        .filter(|value| !value.is_empty())
}
