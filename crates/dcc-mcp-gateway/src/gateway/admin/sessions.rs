//! GET /admin/api/sessions — session list endpoint.

use std::collections::HashMap;

use axum::Json;
use axum::extract::State;
use dcc_mcp_models::session::{Session, SessionStatus};
use serde::Serialize;

use super::state::AdminState;

/// Payload returned by GET /admin/api/sessions.
#[derive(Debug, Serialize)]
pub struct SessionsPayload {
    pub sessions: Vec<Session>,
    pub total: usize,
    pub active: usize,
    pub ended: usize,
    pub by_dcc: HashMap<String, usize>,
    pub by_status: HashMap<String, usize>,
}

/// GET /admin/api/sessions
///
/// Returns all known sessions with summary statistics.
pub async fn handle_admin_sessions(
    State(_state): State<AdminState>,
) -> Json<SessionsPayload> {
    // TODO: query sessions from the SQLite sessions table via admin_sqlite_lane
    // when session persistence is wired into AdminState. For now, return an
    // empty list with zeros.
    let sessions: Vec<Session> = Vec::new();

    let total = sessions.len();
    let active = sessions
        .iter()
        .filter(|s| s.status == SessionStatus::Active)
        .count();
    let ended = total - active;

    let mut by_dcc = HashMap::new();
    let mut by_status = HashMap::new();

    for s in &sessions {
        *by_dcc.entry(s.dcc_type.clone()).or_insert(0) += 1;
        let status_str = format!("{:?}", s.status).to_lowercase();
        *by_status.entry(status_str).or_insert(0) += 1;
    }

    Json(SessionsPayload {
        sessions,
        total,
        active,
        ended,
        by_dcc,
        by_status,
    })
}
