use std::time::Duration;

use axum::Json;
use axum::extract::State;
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::Deserialize;
use serde_json::json;

use super::skill_reload::reload_skill_paths_and_refresh_backends;
use super::state::AdminState;
use crate::gateway::capability::RefreshReason;
use crate::gateway::event_log::{ContendEvent, EventKind};

#[derive(Debug, Deserialize)]
pub struct SkillPathAddBody {
    pub path: String,
}

async fn wait_for_custom_skill_path_visible(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    needle: &str,
) {
    for _ in 0..80 {
        if lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(_, p)| p == needle)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(path = %needle, "skill path not visible after 2 s poll — writer may be lagging");
}

async fn wait_until_custom_skill_path_id_removed(
    lane: &crate::gateway::admin::sqlite_lane::AdminSqliteLane,
    id: i64,
) {
    for _ in 0..80 {
        if !lane
            .reader()
            .list_custom_skill_paths()
            .iter()
            .any(|(i, _)| *i == id)
        {
            return;
        }
        tokio::time::sleep(Duration::from_millis(25)).await;
    }
    tracing::warn!(
        skill_path_id = id,
        "skill path id not removed after 2 s poll — writer may be lagging"
    );
}

fn push_admin_operator_note(state: &AdminState, msg: String) {
    state.gateway.event_log.push(ContendEvent::new(
        EventKind::OperatorNote,
        "admin",
        "gateway",
        Some(msg),
    ));
}

/// `GET /admin/api/skill-paths` — skill search paths (snapshot + SQLite custom).
pub async fn handle_admin_skill_paths(State(s): State<AdminState>) -> impl IntoResponse {
    Json(crate::gateway::admin::skill_health::build_skill_paths_payload(&s))
}

/// `POST /admin/api/skill-paths` — enqueue a custom path; embedder hook may reload disk catalog.
pub async fn handle_admin_skill_path_add(
    State(s): State<AdminState>,
    Json(body): Json<SkillPathAddBody>,
) -> impl IntoResponse {
    let path = body.path.trim().to_string();
    if path.is_empty() {
        return (
            StatusCode::BAD_REQUEST,
            Json(json!({ "error": "path is empty" })),
        )
            .into_response();
    }
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_add_skill_path(path.clone()) {
        wait_for_custom_skill_path_visible(lane, &path).await;
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
        push_admin_operator_note(
            &s,
            format!("Custom skill path persisted; catalog reload hook ran: {path}"),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "path": path }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}

/// `DELETE /admin/api/skill-paths/{id}` — remove a custom path row.
pub async fn handle_admin_skill_path_delete(
    State(s): State<AdminState>,
    axum::extract::Path(id): axum::extract::Path<i64>,
) -> impl IntoResponse {
    let Some(ref lane) = s.admin_sqlite_lane else {
        return (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "admin sqlite lane disabled" })),
        )
            .into_response();
    };
    if lane.try_delete_skill_path(id) {
        wait_until_custom_skill_path_id_removed(lane, id).await;
        reload_skill_paths_and_refresh_backends(&s, RefreshReason::ToolsListChanged).await;
        push_admin_operator_note(
            &s,
            format!("Custom skill path removed (id={id}); catalog reload hook ran."),
        );
        (StatusCode::OK, Json(json!({ "ok": true, "id": id }))).into_response()
    } else {
        (
            StatusCode::SERVICE_UNAVAILABLE,
            Json(json!({ "error": "persist queue full or sqlite disabled" })),
        )
            .into_response()
    }
}
