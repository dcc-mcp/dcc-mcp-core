//! Session API handlers (PIP-2751).
//!
//! Provides `GET /admin/api/sessions` and `GET /admin/api/sessions/{session_id}`
//! backed by the SQLite sessions / tool_calls / session_events tables.
//!
//! The stable `GET /admin/api/sessions` response contract types
//! (`SessionRow` / `SessionKpi` / `SessionsPayload`) mirror the frontend types
//! in `admin-ui/src/admin-types.ts`.

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use dcc_mcp_gateway_admin::{session_detail_payload, sessions_payload};
use serde::Deserialize;
use serde_json::json;

use super::super::state::AdminState;

#[derive(Debug, Default, Deserialize)]
pub struct SessionsQuery {
    /// Filter by DCC type (e.g. `"maya"`).
    dcc_type: Option<String>,
    /// Filter by session status (e.g. `"active"`).
    status: Option<String>,
    /// Max rows to return (default 200).
    limit: Option<usize>,
}

/// `GET /admin/api/sessions` — list sessions with optional filters.
///
/// Returns `{ sessions: [...], total, summary: { active, ended, ... } }`.
pub async fn handle_admin_sessions(
    State(s): State<AdminState>,
    Query(params): Query<SessionsQuery>,
) -> impl IntoResponse {
    let limit = params.limit.unwrap_or(200).clamp(1, 1_000);

    let rows = s
        .admin_sqlite_lane
        .as_ref()
        .map(|lane| {
            lane.reader()
                .list_sessions(limit, params.dcc_type.as_deref(), params.status.as_deref())
        })
        .unwrap_or_default();

    Json(sessions_payload(rows)).into_response()
}

/// `GET /admin/api/sessions/{session_id}` — detail for one session.
///
/// Includes the session record, its tool calls, and lifecycle events.
pub async fn handle_admin_session_detail(
    State(s): State<AdminState>,
    Path(session_id): Path<String>,
) -> impl IntoResponse {
    let lane = match &s.admin_sqlite_lane {
        Some(lane) => lane,
        None => {
            return (
                StatusCode::SERVICE_UNAVAILABLE,
                Json(json!({
                    "error": "sqlite_not_available",
                    "message": "SQLite persistence is not enabled.",
                })),
            )
                .into_response();
        }
    };

    let reader = lane.reader();
    let Some(session) = reader.get_session(&session_id) else {
        return (
            StatusCode::NOT_FOUND,
            Json(json!({
                "error": "session_not_found",
                "message": format!("No session found with id '{session_id}'."),
                "session_id": session_id,
            })),
        )
            .into_response();
    };

    let tool_calls = reader.list_tool_calls(&session_id, 500);
    let events = reader.list_session_events(&session_id, 200);

    Json(session_detail_payload(session, tool_calls, events)).into_response()
}
