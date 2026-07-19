//! Session API handlers (PIP-2751).
//!
//! Provides `GET /admin/api/sessions` and `GET /admin/api/sessions/{session_id}`
//! backed by the SQLite sessions / tool_calls / session_events tables.
//!
//! The stable `GET /admin/api/sessions` response contract types
//! (`SessionRow` / `SessionKpi` / `SessionsPayload`) mirror the frontend types
//! in `admin-ui/src/admin-types.ts`.

use std::collections::{BTreeMap, HashMap};

use axum::Json;
use axum::extract::{Path, Query, State};
use axum::http::StatusCode;
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};
use serde_json::json;

use super::state::AdminState;

// ── Stable response contract types (PIP-2752 frontend contract) ─────────────

/// Correlation ids for one session row — mirrors `SessionRow.correlation` in
/// `admin-ui/src/admin-types.ts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionCorrelation {
    #[serde(skip_serializing_if = "Option::is_none")]
    pub request_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub trace_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub workflow_id: Option<String>,
}

/// One row in the sessions table — mirrors `SessionRow` in
/// `admin-ui/src/admin-types.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionRow {
    pub session_id: String,
    pub parent_session_id: Option<String>,
    /// One of `active`, `ended`, `crashed`, `interrupted`, `timed_out`, `cancelled`, `unknown`.
    pub status: String,
    pub dcc_type: Option<String>,
    pub instance_id: Option<String>,
    pub agent_id: Option<String>,
    pub agent_name: Option<String>,
    pub agent_model: Option<String>,
    pub started_at: String,
    pub ended_at: Option<String>,
    pub duration_ms: Option<u64>,
    pub turn_count: u64,
    pub tool_call_count: u64,
    pub end_reason: Option<String>,
    pub version: Option<String>,
    pub actor_id: Option<String>,
    pub actor_name: Option<String>,
    pub correlation: SessionCorrelation,
}

/// Aggregate KPI summary — mirrors `SessionKpi` in `admin-ui/src/admin-types.ts`.
#[derive(Debug, Clone, Default, Serialize)]
pub struct SessionKpi {
    pub total: u64,
    pub active: u64,
    pub ended: u64,
    pub crashed: u64,
    pub by_dcc: BTreeMap<String, u64>,
}

/// Response body for `GET /admin/api/sessions` — mirrors `SessionsPayload` in
/// `admin-ui/src/admin-types.ts`.
#[derive(Debug, Clone, Serialize)]
pub struct SessionsPayload {
    pub sessions: Vec<SessionRow>,
    pub kpi: SessionKpi,
    pub total: usize,
}

// ── SQLite-backed handlers (PIP-2751) ───────────────────────────────────────

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

    let mut by_dcc: HashMap<String, usize> = HashMap::new();
    let mut active_count = 0usize;
    let mut ended_count = 0usize;
    let mut crashed_count = 0usize;
    let mut disconnected_count = 0usize;

    for row in &rows {
        let dcc = row
            .get("dcc_type")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        *by_dcc.entry(dcc.to_string()).or_default() += 1;

        let status = row
            .get("status")
            .and_then(|v| v.as_str())
            .unwrap_or("unknown");
        match status {
            "active" => active_count += 1,
            "ended" => ended_count += 1,
            "crashed" | "gpu_crashed" => crashed_count += 1,
            "disconnected" => disconnected_count += 1,
            _ => {}
        }
    }

    Json(json!({
        "total": rows.len(),
        "sessions": rows,
        "summary": {
            "active": active_count,
            "ended": ended_count,
            "crashed": crashed_count,
            "disconnected": disconnected_count,
            "by_dcc": by_dcc,
        },
    }))
    .into_response()
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

    // Compute tool-call stats
    let total_calls = tool_calls.len();
    let successful_calls = tool_calls
        .iter()
        .filter(|tc| tc.get("success").and_then(|v| v.as_i64()) == Some(1))
        .count();
    let failed_calls = total_calls.saturating_sub(successful_calls);

    Json(json!({
        "session": session,
        "tool_calls": tool_calls,
        "events": events,
        "summary": {
            "total_tool_calls": total_calls,
            "successful_tool_calls": successful_calls,
            "failed_tool_calls": failed_calls,
        },
    }))
    .into_response()
}
