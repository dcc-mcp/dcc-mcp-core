//! Session, tool-call, and experiment projections for the gateway admin SQLite store.
//!
//! These reads are all projections over the shared session event timeline. They
//! live apart from `gateway_admin_sqlite` so both files stay inside the
//! 1 500-line production Rust limit enforced by
//! `.github/workflows/check-file-size.yml`.

use rusqlite::{Connection, ToSql, params, params_from_iter};
use serde_json::json;

/// PIP-2751: List sessions, newest first, with optional filters.
pub(super) fn list_sessions_json(
    conn: &Connection,
    limit: usize,
    dcc_type: Option<&str>,
    status: Option<&str>,
) -> Vec<String> {
    let mut sql = String::from(
        "SELECT session_id, parent_session_id, dcc_type, instance_id, status, \
         started_at_ms, last_activity_at_ms, ended_at_ms, end_reason_json, \
         tool_call_count, error_count, core_version, adapter_version, build_sha \
         FROM sessions WHERE 1 = 1",
    );
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(value) = non_empty(dcc_type) {
        sql.push_str(" AND dcc_type = ?");
        values.push(Box::new(value.to_owned()));
    }
    if let Some(value) = non_empty(status) {
        sql.push_str(" AND status = ?");
        values.push(Box::new(value.to_owned()));
    }
    sql.push_str(" ORDER BY started_at_ms DESC LIMIT ?");
    values.push(Box::new(limit.clamp(1, 10_000) as i64));
    let refs: Vec<&dyn ToSql> = values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params_from_iter(refs), |row| {
        Ok(json!({
            "session_id": row.get::<_, String>(0)?,
            "parent_session_id": row.get::<_, Option<String>>(1)?,
            "dcc_type": row.get::<_, String>(2)?,
            "instance_id": row.get::<_, Option<String>>(3)?,
            "status": row.get::<_, String>(4)?,
            "started_at_ms": row.get::<_, i64>(5)?,
            "last_activity_at_ms": row.get::<_, i64>(6)?,
            "ended_at_ms": row.get::<_, Option<i64>>(7)?,
            "end_reason_json": row.get::<_, Option<String>>(8)?,
            "tool_call_count": row.get::<_, i64>(9)?,
            "error_count": row.get::<_, i64>(10)?,
            "core_version": row.get::<_, String>(11)?,
            "adapter_version": row.get::<_, Option<String>>(12)?,
            "build_sha": row.get::<_, Option<String>>(13)?,
        })
        .to_string())
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|row| row.ok()).collect()
}

/// PIP-2751: Get a single session by id.
pub(super) fn get_session_json(conn: &Connection, session_id: &str) -> Option<String> {
    conn.query_row(
        "SELECT session_id, parent_session_id, dcc_type, instance_id, status, \
         started_at_ms, last_activity_at_ms, ended_at_ms, end_reason_json, \
         tool_call_count, error_count, core_version, adapter_version, build_sha \
         FROM sessions WHERE session_id = ?1",
        params![session_id],
        |row| {
            Ok(json!({
                "session_id": row.get::<_, String>(0)?,
                "parent_session_id": row.get::<_, Option<String>>(1)?,
                "dcc_type": row.get::<_, String>(2)?,
                "instance_id": row.get::<_, Option<String>>(3)?,
                "status": row.get::<_, String>(4)?,
                "started_at_ms": row.get::<_, i64>(5)?,
                "last_activity_at_ms": row.get::<_, i64>(6)?,
                "ended_at_ms": row.get::<_, Option<i64>>(7)?,
                "end_reason_json": row.get::<_, Option<String>>(8)?,
                "tool_call_count": row.get::<_, i64>(9)?,
                "error_count": row.get::<_, i64>(10)?,
                "core_version": row.get::<_, String>(11)?,
                "adapter_version": row.get::<_, Option<String>>(12)?,
                "build_sha": row.get::<_, Option<String>>(13)?,
            })
            .to_string())
        },
    )
    .ok()
}

/// PIP-2751: List session events for a given session, newest first.
pub(super) fn list_session_events_json(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> Vec<String> {
    let mut stmt = match conn.prepare_cached(
        "SELECT event_json FROM session_events WHERE session_id = ?1 \
         ORDER BY created_at_ms DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![session_id, limit.clamp(1, 1_000) as i64], |row| {
        let s: String = row.get(0)?;
        Ok(s)
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Read one recording's ordered projection from the shared session timeline.
pub(super) fn list_recording_events_json(
    conn: &Connection,
    session_id: &str,
    recording_id: &str,
    limit: usize,
) -> Vec<String> {
    let mut stmt = match conn.prepare_cached(
        "SELECT event_json FROM session_events \
         WHERE session_id = ?1 AND event_type LIKE 'recording.%' \
         AND json_extract(event_json, '$.recording_id') = ?2 \
         ORDER BY id ASC LIMIT ?3",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(
        params![session_id, recording_id, limit.clamp(1, 2_000) as i64],
        |row| row.get(0),
    );
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// Return recording starts whose latest lifecycle event is still `started`.
pub(super) fn list_unfinished_recording_starts_json(
    conn: &Connection,
    limit: usize,
) -> Vec<String> {
    let mut stmt = match conn.prepare_cached(
        "SELECT current.event_json FROM session_events AS current \
         JOIN ( \
           SELECT json_extract(event_json, '$.recording_id') AS recording_id, MAX(id) AS latest_id \
           FROM session_events \
           WHERE event_type IN ('recording.started', 'recording.stopped', 'recording.interrupted') \
           GROUP BY recording_id \
         ) AS latest ON latest.latest_id = current.id \
         WHERE current.event_type = 'recording.started' \
         ORDER BY current.id DESC LIMIT ?1",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![limit.clamp(1, 1_000) as i64], |row| row.get(0));
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// List experiment definitions from the existing session event timeline.
pub(super) fn list_experiments_json(conn: &Connection, limit: usize) -> Vec<String> {
    let mut stmt = match conn.prepare_cached(
        "SELECT event_json FROM session_events WHERE event_type = 'experiment.created' \
         ORDER BY created_at_ms DESC, id DESC LIMIT ?1",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![limit.clamp(1, 1_000) as i64], |row| row.get(0));
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(Result::ok).collect()
}

/// Project all events for one experiment from the shared session timeline.
pub(super) fn list_experiment_events_json(
    conn: &Connection,
    experiment_id: &str,
    limit: usize,
) -> Vec<String> {
    // ponytail: bounded scan avoids a second projection table; add an indexed
    // experiment_id column only after retained event volume makes this measurable.
    // The predicate must live in SQL ahead of LIMIT: filtering in Rust after a
    // bounded scan would drop an experiment's newest events whenever other
    // experiments contributed more rows than the scan window.
    let mut stmt = match conn.prepare_cached(
        "SELECT event_json FROM session_events \
         WHERE event_type LIKE 'experiment.%' \
         AND json_extract(event_json, '$.experiment_id') = ?1 \
         ORDER BY created_at_ms DESC, id DESC LIMIT ?2",
    ) {
        Ok(stmt) => stmt,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(
        params![experiment_id, limit.clamp(1, 1_000) as i64],
        |row| row.get::<_, String>(0),
    );
    let Ok(rows) = rows else {
        return Vec::new();
    };
    let mut events = rows.filter_map(Result::ok).collect::<Vec<_>>();
    events.reverse();
    events
}

/// PIP-2751: List tool calls for a given session, newest first.
pub(super) fn list_tool_calls_json(
    conn: &Connection,
    session_id: &str,
    limit: usize,
) -> Vec<String> {
    let mut stmt = match conn.prepare_cached(
        "SELECT request_id, session_id, parent_request_id, batch_id, tool_name, skill_name, \
         dcc_type, instance_id, agent_id, transport, via_gateway, started_at_ms, \
         duration_ms, success, error_message, error_kind, mcp_method, trace_id, span_id \
         FROM tool_calls WHERE session_id = ?1 \
         ORDER BY started_at_ms DESC LIMIT ?2",
    ) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params![session_id, limit.clamp(1, 1_000) as i64], |row| {
        Ok(json!({
            "request_id": row.get::<_, String>(0)?,
            "session_id": row.get::<_, String>(1)?,
            "parent_request_id": row.get::<_, Option<String>>(2)?,
            "batch_id": row.get::<_, Option<String>>(3)?,
            "tool_name": row.get::<_, String>(4)?,
            "skill_name": row.get::<_, Option<String>>(5)?,
            "dcc_type": row.get::<_, Option<String>>(6)?,
            "instance_id": row.get::<_, Option<String>>(7)?,
            "agent_id": row.get::<_, Option<String>>(8)?,
            "transport": row.get::<_, Option<String>>(9)?,
            "via_gateway": row.get::<_, Option<i64>>(10)?,
            "started_at_ms": row.get::<_, i64>(11)?,
            "duration_ms": row.get::<_, i64>(12)?,
            "success": row.get::<_, i64>(13)?,
            "error_message": row.get::<_, Option<String>>(14)?,
            "error_kind": row.get::<_, Option<String>>(15)?,
            "mcp_method": row.get::<_, Option<String>>(16)?,
            "trace_id": row.get::<_, Option<String>>(17)?,
            "span_id": row.get::<_, Option<String>>(18)?,
        })
        .to_string())
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// PIP-2751: List all tool calls, newest first, with optional session filter.
pub(super) fn list_all_tool_calls_json(
    conn: &Connection,
    limit: usize,
    session_id: Option<&str>,
) -> Vec<String> {
    let mut sql = String::from(
        "SELECT request_id, session_id, parent_request_id, batch_id, tool_name, skill_name, \
         dcc_type, instance_id, agent_id, transport, via_gateway, started_at_ms, \
         duration_ms, success, error_message, error_kind, mcp_method, trace_id, span_id \
         FROM tool_calls WHERE 1 = 1",
    );
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(value) = non_empty(session_id) {
        sql.push_str(" AND session_id = ?");
        values.push(Box::new(value.to_owned()));
    }
    sql.push_str(" ORDER BY started_at_ms DESC LIMIT ?");
    values.push(Box::new(limit.clamp(1, 10_000) as i64));
    let refs: Vec<&dyn ToSql> = values.iter().map(|value| value.as_ref()).collect();
    let mut stmt = match conn.prepare_cached(&sql) {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map(params_from_iter(refs), |row| {
        Ok(json!({
            "request_id": row.get::<_, String>(0)?,
            "session_id": row.get::<_, String>(1)?,
            "parent_request_id": row.get::<_, Option<String>>(2)?,
            "batch_id": row.get::<_, Option<String>>(3)?,
            "tool_name": row.get::<_, String>(4)?,
            "skill_name": row.get::<_, Option<String>>(5)?,
            "dcc_type": row.get::<_, Option<String>>(6)?,
            "instance_id": row.get::<_, Option<String>>(7)?,
            "agent_id": row.get::<_, Option<String>>(8)?,
            "transport": row.get::<_, Option<String>>(9)?,
            "via_gateway": row.get::<_, Option<i64>>(10)?,
            "started_at_ms": row.get::<_, i64>(11)?,
            "duration_ms": row.get::<_, i64>(12)?,
            "success": row.get::<_, i64>(13)?,
            "error_message": row.get::<_, Option<String>>(14)?,
            "error_kind": row.get::<_, Option<String>>(15)?,
            "mcp_method": row.get::<_, Option<String>>(16)?,
            "trace_id": row.get::<_, Option<String>>(17)?,
            "span_id": row.get::<_, Option<String>>(18)?,
        })
        .to_string())
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

/// Treat whitespace-only filters as absent so callers can pass raw user input.
fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}
