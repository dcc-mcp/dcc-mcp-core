//! Admin API handler for DCC/agent session tracking.
//!
//! Defines the stable `GET /admin/api/sessions` response contract that the
//! `admin-ui` frontend expects (`SessionRow` / `SessionKpi` / `SessionsPayload`
//! in `admin-ui/src/admin-types.ts`).
//!
//! No durable session store is wired into [`AdminState`] yet, so this handler
//! currently returns an empty, correctly-shaped payload with a placeholder
//! KPI summary. See the `TODO(sessions)` note on [`collect_sessions`] for
//! where real backend integration should land.

use std::collections::BTreeMap;

use axum::Json;
use axum::extract::{Query, State};
use axum::response::IntoResponse;
use serde::{Deserialize, Serialize};

use super::state::AdminState;

/// Query parameters accepted by `GET /admin/api/sessions`.
#[derive(Debug, Default, Deserialize)]
pub struct SessionQuery {
    /// Filter to sessions for a specific DCC type (e.g. `"maya"`, `"blender"`).
    pub dcc_type: Option<String>,
    /// Filter to sessions with a specific status (`active`, `ended`, `crashed`, ...).
    pub status: Option<String>,
    /// Free-text search across session id / agent / actor / instance fields.
    pub search: Option<String>,
    /// Maximum number of rows to return after filtering. Defaults to 200, capped at 1000.
    pub limit: Option<usize>,
    /// Number of filtered rows to skip before applying `limit` (pagination).
    pub offset: Option<usize>,
}

impl SessionQuery {
    fn limit(&self) -> usize {
        self.limit.unwrap_or(200).clamp(1, 1_000)
    }

    fn offset(&self) -> usize {
        self.offset.unwrap_or(0)
    }

    fn non_empty(value: Option<&str>) -> Option<String> {
        value
            .map(str::trim)
            .filter(|value| !value.is_empty())
            .map(|value| value.to_ascii_lowercase())
    }
}

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

/// `GET /admin/api/sessions` — DCC/agent session inventory + KPI summary.
///
/// Supports `?dcc_type=`, `?status=`, `?search=`, `?limit=`, `?offset=`.
pub async fn handle_admin_sessions(
    State(state): State<AdminState>,
    Query(params): Query<SessionQuery>,
) -> impl IntoResponse {
    Json(build_sessions_payload(&state, &params).await)
}

/// Build the sessions payload from currently retained gateway state.
pub async fn build_sessions_payload(state: &AdminState, params: &SessionQuery) -> SessionsPayload {
    let rows = collect_sessions(state).await;
    let kpi = compute_kpi(&rows);
    let filtered = filter_sessions(rows, params);
    let total = filtered.len();
    let sessions = paginate(filtered, params.offset(), params.limit());
    SessionsPayload {
        sessions,
        kpi,
        total,
    }
}

/// Read all known session rows from the gateway/admin state.
///
/// TODO(sessions): `AdminState` has no durable session store yet — session
/// lifecycle (start/turn/end/crash) is not currently persisted anywhere the
/// admin API can read from. `AdminAuditRecord` and `DispatchTrace` carry a
/// bare `session_id` string per call, but no dedicated start/end/status
/// record. Once session tracking lands (e.g. a `sessions` table in the
/// `AdminSqliteLane`/`AdminSqliteReader`, or an in-memory ring buffer similar
/// to `AuditLog`/`TraceLog` attached to `AdminState`), replace this stub:
///   1. Read session rows from that store, most-recent first.
///   2. Map them into [`SessionRow`].
///
/// The filtering/KPI/pagination logic below already operates on `Vec<SessionRow>`
/// and requires no changes once real data is available.
async fn collect_sessions(_state: &AdminState) -> Vec<SessionRow> {
    Vec::new()
}

/// Compute the KPI summary over the full (unfiltered) session set.
fn compute_kpi(rows: &[SessionRow]) -> SessionKpi {
    let mut kpi = SessionKpi {
        total: rows.len() as u64,
        ..SessionKpi::default()
    };
    for row in rows {
        match row.status.as_str() {
            "active" => kpi.active += 1,
            "ended" => kpi.ended += 1,
            "crashed" => kpi.crashed += 1,
            _ => {}
        }
        if let Some(dcc_type) = row.dcc_type.as_deref().filter(|v| !v.is_empty()) {
            *kpi.by_dcc.entry(dcc_type.to_string()).or_insert(0) += 1;
        }
    }
    kpi
}

/// Apply `dcc_type` / `status` / `search` query filters to the session rows.
fn filter_sessions(rows: Vec<SessionRow>, params: &SessionQuery) -> Vec<SessionRow> {
    let dcc_type = SessionQuery::non_empty(params.dcc_type.as_deref());
    let status = SessionQuery::non_empty(params.status.as_deref());
    let search = SessionQuery::non_empty(params.search.as_deref());

    rows.into_iter()
        .filter(|row| {
            dcc_type.as_deref().is_none_or(|filter| {
                row.dcc_type
                    .as_deref()
                    .is_some_and(|value| value.eq_ignore_ascii_case(filter))
            })
        })
        .filter(|row| {
            status
                .as_deref()
                .is_none_or(|filter| row.status.eq_ignore_ascii_case(filter))
        })
        .filter(|row| {
            search
                .as_deref()
                .is_none_or(|needle| session_matches_search(row, needle))
        })
        .collect()
}

fn session_matches_search(row: &SessionRow, needle: &str) -> bool {
    let haystacks = [
        Some(row.session_id.as_str()),
        row.agent_id.as_deref(),
        row.agent_name.as_deref(),
        row.actor_id.as_deref(),
        row.actor_name.as_deref(),
        row.instance_id.as_deref(),
    ];
    haystacks
        .into_iter()
        .flatten()
        .any(|value| value.to_ascii_lowercase().contains(needle))
}

/// Apply `offset`/`limit` pagination to the already-filtered rows.
fn paginate(rows: Vec<SessionRow>, offset: usize, limit: usize) -> Vec<SessionRow> {
    rows.into_iter().skip(offset).take(limit).collect()
}
