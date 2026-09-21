//! Gateway admin SQLite reader + writer thread (traces, audits, custom skill paths).
//!
//! JSON blobs for traces/audits are opaque at this layer; the gateway deserialises
//! into its own trace/audit types.

use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, SyncSender, sync_channel};
use std::sync::{Arc, Mutex};
use std::thread::JoinHandle;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use rusqlite::{Connection, ToSql, params, params_from_iter};
use serde::Deserialize;
use serde_json::json;

use crate::domain::feedback_report::{FeedbackReportInsert, FeedbackReportRow};
use crate::domain::gateway_admin_audit::GatewayAdminAuditPersistedJson;
use crate::domain::gateway_admin_deregistered::GatewayDeregisteredInstanceJson;
use crate::domain::script_promotion::ScriptPromotionBumpJson;
use crate::infra::feedback_report_sqlite::{
    insert_feedback_report, list_feedback_reports_json, prune_feedback_reports,
};
use crate::infra::gateway_admin_schema::GATEWAY_ADMIN_SQLITE_DDL;
use crate::infra::script_promotion_sqlite::{
    bump_script_promotion_counter, get_script_promotion_counter_json,
    list_script_promotion_counters_json,
};

const SCHEMA: &str = GATEWAY_ADMIN_SQLITE_DDL;

#[derive(Deserialize)]
struct TraceInsertMeta {
    request_id: String,
    started_at: u64,
}

#[derive(Clone)]
pub struct GatewayAdminSqliteReader {
    path: PathBuf,
}

impl GatewayAdminSqliteReader {
    #[must_use]
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    fn open_ro(&self) -> Option<Connection> {
        Connection::open_with_flags(
            &self.path,
            rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
        )
        .ok()
    }

    /// Raw `trace_json` rows, newest first, bounded by `limit`.
    pub fn list_traces_since_json(&self, cutoff: Option<SystemTime>, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let cutoff_ms = cutoff
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut stmt = match conn.prepare_cached(
            "SELECT trace_json FROM traces WHERE started_ms >= ?1 ORDER BY started_ms DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![cutoff_ms, limit as i64], |row| {
            let s: String = row.get(0)?;
            Ok(s)
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn get_trace_json(&self, request_id: &str) -> Option<String> {
        let conn = self.open_ro()?;
        conn.query_row(
            "SELECT trace_json FROM traces WHERE request_id = ?1",
            params![request_id],
            |row| row.get(0),
        )
        .ok()
    }

    pub fn list_audits_recent_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut stmt = match conn
            .prepare_cached("SELECT audit_json FROM audits ORDER BY ts_ms DESC LIMIT ?1")
        {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![limit as i64], |row| {
            let s: String = row.get(0)?;
            Ok(s)
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Raw `audit_json` rows with `ts_ms >= cutoff_ms`, newest first, bounded by `limit`.
    pub fn list_audits_since_json(&self, cutoff: Option<SystemTime>, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let cutoff_ms = cutoff
            .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
            .map(|d| d.as_millis() as i64)
            .unwrap_or(0);
        let mut stmt = match conn.prepare_cached(
            "SELECT audit_json FROM audits WHERE ts_ms >= ?1 ORDER BY ts_ms DESC LIMIT ?2",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![cutoff_ms, limit as i64], |row| {
            let s: String = row.get(0)?;
            Ok(s)
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn list_custom_skill_paths(&self) -> Vec<(i64, String)> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut stmt =
            match conn.prepare_cached("SELECT id, path FROM skill_paths_custom ORDER BY id ASC") {
                Ok(s) => s,
                Err(_) => return Vec::new(),
            };
        let rows = stmt.query_map([], |row| {
            Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?))
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn list_deregistered_instances_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut stmt = match conn.prepare_cached(
            "SELECT entry_json FROM deregistered_instances ORDER BY ts_ms DESC, id DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![limit as i64], |row| {
            let s: String = row.get(0)?;
            Ok(s)
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// PIP-2751: List sessions, newest first, with optional filters.
    pub fn list_sessions_json(
        &self,
        limit: usize,
        dcc_type: Option<&str>,
        status: Option<&str>,
    ) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn get_session_json(&self, session_id: &str) -> Option<String> {
        let conn = self.open_ro()?;
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
    pub fn list_session_events_json(&self, session_id: &str, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn list_recording_events_json(
        &self,
        session_id: &str,
        recording_id: &str,
        limit: usize,
    ) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn list_unfinished_recording_starts_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn list_experiments_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn list_experiment_events_json(&self, experiment_id: &str, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        // ponytail: bounded scan avoids a second projection table; add an indexed
        // experiment_id column only after retained event volume makes this measurable.
        let mut stmt = match conn.prepare_cached(
            "SELECT event_json FROM session_events WHERE event_type LIKE 'experiment.%' \
             ORDER BY created_at_ms DESC, id DESC LIMIT 10000",
        ) {
            Ok(stmt) => stmt,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map([], |row| row.get::<_, String>(0));
        let Ok(rows) = rows else {
            return Vec::new();
        };
        let mut events = rows
            .filter_map(Result::ok)
            .filter(|event| {
                serde_json::from_str::<serde_json::Value>(event)
                    .ok()
                    .and_then(|value| {
                        value
                            .get("experiment_id")
                            .and_then(|value| value.as_str())
                            .map(|value| value == experiment_id)
                    })
                    .unwrap_or(false)
            })
            .take(limit.clamp(1, 1_000))
            .collect::<Vec<_>>();
        events.reverse();
        events
    }

    /// PIP-2751: List tool calls for a given session, newest first.
    pub fn list_tool_calls_json(&self, session_id: &str, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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
    pub fn list_all_tool_calls_json(&self, limit: usize, session_id: Option<&str>) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
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

    /// Most recently seen feedback reports, newest first, bounded by `limit`.
    pub fn list_feedback_reports_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut stmt = match conn.prepare_cached(
            "SELECT report_json FROM feedback_reports ORDER BY last_seen_ms DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![limit as i64], |row| row.get::<_, String>(0));
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    /// Look up one report by its `(repo, fingerprint)` dedup key.
    pub fn get_feedback_report(&self, repo: &str, fingerprint: &str) -> Option<FeedbackReportRow> {
        let conn = self.open_ro()?;
        select_feedback_report(&conn, repo, fingerprint).ok()
    }

    /// Most recently seen reports, newest first, bounded by `limit`.
    pub fn list_feedback_reports(&self, limit: usize) -> Vec<FeedbackReportRow> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut stmt = match conn.prepare_cached(
            "SELECT id, repo, fingerprint, issues_url, route_rationale, dcc_type, phase, severity, \
             first_seen_ms, last_seen_ms, occurrence_count, report_json \
             FROM feedback_reports ORDER BY last_seen_ms DESC LIMIT ?1",
        ) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params![limit as i64], row_to_feedback_report);
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|r| r.ok()).collect()
    }

    pub fn list_agent_memory_json(
        &self,
        layer: Option<&str>,
        dcc_name: Option<&str>,
        session_id: Option<&str>,
        key_prefix: Option<&str>,
        limit: usize,
    ) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        let mut sql = String::from(
            "SELECT id, layer, key, session_id, dcc_name, score, created_unix_secs, payload_json \
             FROM agent_memory WHERE 1 = 1",
        );
        let mut values: Vec<Box<dyn ToSql>> = Vec::new();
        if let Some(value) = non_empty(layer) {
            sql.push_str(" AND layer = ?");
            values.push(Box::new(value.to_owned()));
        }
        if let Some(value) = non_empty(dcc_name) {
            sql.push_str(" AND dcc_name = ?");
            values.push(Box::new(value.to_owned()));
        }
        if let Some(value) = non_empty(session_id) {
            sql.push_str(" AND session_id = ?");
            values.push(Box::new(value.to_owned()));
        }
        if let Some(value) = non_empty(key_prefix) {
            sql.push_str(r" AND key LIKE ? ESCAPE '\'");
            values.push(Box::new(sqlite_like_prefix(value)));
        }
        sql.push_str(" ORDER BY created_unix_secs DESC, score DESC, id DESC LIMIT ?");
        values.push(Box::new(limit.clamp(1, 1_000) as i64));
        let refs: Vec<&dyn ToSql> = values.iter().map(|value| value.as_ref()).collect();
        let mut stmt = match conn.prepare_cached(&sql) {
            Ok(s) => s,
            Err(_) => return Vec::new(),
        };
        let rows = stmt.query_map(params_from_iter(refs), |row| {
            let payload_json: String = row.get(7)?;
            let payload =
                serde_json::from_str::<serde_json::Value>(&payload_json).unwrap_or_default();
            Ok(json!({
                "id": row.get::<_, i64>(0)?,
                "layer": row.get::<_, String>(1)?,
                "key": row.get::<_, String>(2)?,
                "session_id": row.get::<_, String>(3)?,
                "dcc_name": row.get::<_, String>(4)?,
                "score": row.get::<_, f64>(5)?,
                "created_unix_secs": row.get::<_, f64>(6)?,
                "payload": payload,
            })
            .to_string())
        });
        let Ok(rows) = rows else {
            return Vec::new();
        };
        rows.filter_map(|row| row.ok()).collect()
    }

    /// #2297-A3: Read one persisted repeat counter as JSON.
    #[must_use]
    pub fn get_script_promotion_counter_json(
        &self,
        sha256: &str,
        dcc_type: &str,
        tool_name: &str,
    ) -> Option<String> {
        let conn = self.open_ro()?;
        get_script_promotion_counter_json(&conn, sha256, dcc_type, tool_name)
            .ok()
            .flatten()
    }

    /// #2297-A3: Read repeat counters as JSON, most recently bumped first.
    #[must_use]
    pub fn list_script_promotion_counters_json(&self, limit: usize) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        list_script_promotion_counters_json(&conn, limit).unwrap_or_default()
    }
    /// Raw `report_json` rows for persisted feedback, newest first, bounded by `limit`.
    ///
    /// `cutoff_ms` filters on `occurred_at_ms`; `dcc` / `severity` are
    /// case-insensitive equality filters, matching the JSONL fallback path.
    /// See [`crate::infra::feedback_report_sqlite`] for the SQL.
    #[must_use]
    pub fn list_feedback_reports_json(
        &self,
        cutoff_ms: Option<i64>,
        dcc: Option<&str>,
        severity: Option<&str>,
        limit: usize,
    ) -> Vec<String> {
        let Some(conn) = self.open_ro() else {
            return Vec::new();
        };
        list_feedback_reports_json(&conn, cutoff_ms, dcc, severity, limit).unwrap_or_default()
    }
}

enum PersistMsg {
    TraceJson(String),
    AuditJson(String),
    DeregisteredInstanceJson(String),
    AddSkillPath(String),
    DeleteSkillPath(i64),
    DeleteAgentMemory {
        id: Option<i64>,
        layer: Option<String>,
        dcc_name: Option<String>,
        session_id: Option<String>,
        key_prefix: Option<String>,
    },
    /// PIP-2751: Structured tool-call event (JSON-serialized ToolCallEvent).
    ToolCallEventJson(String),
    /// PIP-2751: Session upsert (JSON-serialized Session).
    SessionUpsertJson(String),
    /// PIP-2751: Session lifecycle event (JSON-serialized).
    SessionEventJson(String),
    /// #2253-E1: Agent feedback report (JSON-serialized).
    FeedbackReportJson(String),
    /// #2297-A3: Repeat-counter bump (JSON-serialized ScriptPromotionBumpJson).
    ScriptPromotionBumpJson(String),
}

struct LaneShared {
    reader: GatewayAdminSqliteReader,
    tx: Mutex<Option<SyncSender<PersistMsg>>>,
    join: Mutex<Option<JoinHandle<()>>>,
}

impl Drop for LaneShared {
    fn drop(&mut self) {
        if let Ok(mut g) = self.tx.lock() {
            g.take();
        }
        if let Ok(mut jg) = self.join.lock()
            && let Some(j) = jg.take()
        {
            let _ = j.join();
        }
    }
}

#[derive(Clone)]
pub struct GatewayAdminSqliteLane {
    inner: Arc<LaneShared>,
}

impl GatewayAdminSqliteLane {
    pub fn spawn(path: PathBuf, retention_days: u32) -> Result<Self, String> {
        if let Some(parent) = path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        {
            let conn = Connection::open(&path).map_err(|e| e.to_string())?;
            conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        }

        let (tx, rx) = sync_channel::<PersistMsg>(8_192);
        let path_thread = path.clone();
        let join = std::thread::Builder::new()
            .name("dcc-mcp-admin-sqlite".into())
            .spawn(move || writer_main(path_thread, retention_days, rx))
            .map_err(|e| e.to_string())?;

        Ok(Self {
            inner: Arc::new(LaneShared {
                reader: GatewayAdminSqliteReader::new(path),
                tx: Mutex::new(Some(tx)),
                join: Mutex::new(Some(join)),
            }),
        })
    }

    #[must_use]
    pub fn reader(&self) -> GatewayAdminSqliteReader {
        self.inner.reader.clone()
    }

    pub fn try_persist_trace_json(&self, trace_json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::TraceJson(trace_json.to_owned()));
        }
    }

    pub fn try_persist_audit_json(&self, audit_json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::AuditJson(audit_json.to_owned()));
        }
    }

    pub fn try_persist_deregistered_instance_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::DeregisteredInstanceJson(json.to_owned()));
        }
    }

    pub fn try_add_skill_path(&self, path: String) -> bool {
        self.inner
            .tx
            .lock()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .map(|tx| tx.try_send(PersistMsg::AddSkillPath(path)).is_ok())
            })
            .unwrap_or(false)
    }

    pub fn try_delete_skill_path(&self, id: i64) -> bool {
        self.inner
            .tx
            .lock()
            .ok()
            .and_then(|g| {
                g.as_ref()
                    .map(|tx| tx.try_send(PersistMsg::DeleteSkillPath(id)).is_ok())
            })
            .unwrap_or(false)
    }

    pub fn try_delete_agent_memory(
        &self,
        id: Option<i64>,
        layer: Option<String>,
        dcc_name: Option<String>,
        session_id: Option<String>,
        key_prefix: Option<String>,
    ) -> bool {
        self.inner
            .tx
            .lock()
            .ok()
            .and_then(|g| {
                g.as_ref().map(|tx| {
                    tx.try_send(PersistMsg::DeleteAgentMemory {
                        id,
                        layer,
                        dcc_name,
                        session_id,
                        key_prefix,
                    })
                    .is_ok()
                })
            })
            .unwrap_or(false)
    }

    /// PIP-2751: Persist a structured tool-call event.
    pub fn try_persist_tool_call_event_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::ToolCallEventJson(json.to_owned()));
        }
    }

    /// PIP-2751: Upsert a session record.
    pub fn try_upsert_session_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::SessionUpsertJson(json.to_owned()));
        }
    }

    /// PIP-2751: Persist a session lifecycle event.
    pub fn try_persist_session_event_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::SessionEventJson(json.to_owned()));
        }
    }

    /// #2253-E1: Persist an agent feedback report.
    pub fn try_persist_feedback_report_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::FeedbackReportJson(json.to_owned()));
        }
    }

    /// #2297-A3: Record one repeat observation of a materialised script.
    ///
    /// Fire-and-forget: the bump is applied by the writer thread, so the
    /// caller must not block on the resulting count.
    pub fn try_bump_script_promotion_counter_json(&self, json: &str) {
        if let Ok(g) = self.inner.tx.lock()
            && let Some(tx) = g.as_ref()
        {
            let _ = tx.try_send(PersistMsg::ScriptPromotionBumpJson(json.to_owned()));
        }
    }

    /// Insert or collapse one feedback report and return the resulting row.
    ///
    /// Unlike the `try_persist_*` helpers this is **synchronous**: the gateway
    /// needs `occurrence_count` and the row id to build its HTTP response, and
    /// reading them back over the async lane would race the writer thread.
    /// Opens its own connection to the same file (WAL permits concurrent
    /// writers; SQLite serialises them).
    pub fn upsert_feedback_report(
        &self,
        row: &FeedbackReportInsert,
    ) -> Result<FeedbackReportRow, String> {
        let path = self.path();
        let mut conn = Connection::open(path).map_err(|e| e.to_string())?;
        conn.execute_batch(SCHEMA).map_err(|e| e.to_string())?;
        upsert_feedback_report(&mut conn, row)
    }

    /// Filesystem path of the admin SQLite database.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.inner.reader.path
    }

    /// Look up one report by its dedup key without writing.
    #[must_use]
    pub fn get_feedback_report(&self, repo: &str, fingerprint: &str) -> Option<FeedbackReportRow> {
        self.inner.reader.get_feedback_report(repo, fingerprint)
    }

    /// Most recently seen reports, newest first, bounded by `limit`.
    #[must_use]
    pub fn list_feedback_reports(&self, limit: usize) -> Vec<FeedbackReportRow> {
        self.inner.reader.list_feedback_reports(limit)
    }
}

fn writer_main(path: PathBuf, retention_days: u32, rx: Receiver<PersistMsg>) {
    let Ok(mut conn) = Connection::open(&path) else {
        tracing::error!(path = %path.display(), "admin sqlite writer: failed to open DB");
        return;
    };
    let _ = conn.execute_batch(SCHEMA);
    let mut n: u64 = 0;
    while let Ok(msg) = rx.recv() {
        match msg {
            PersistMsg::TraceJson(json) => {
                if let Ok(meta) = serde_json::from_str::<TraceInsertMeta>(&json) {
                    let ms = meta.started_at.min(i64::MAX as u64) as i64;
                    if let Err(e) = conn.execute(
                        "INSERT OR REPLACE INTO traces (request_id, started_ms, trace_json) VALUES (?1, ?2, ?3)",
                        params![meta.request_id, ms, json],
                    ) {
                        tracing::debug!(error = %e, request_id = %meta.request_id, "admin sqlite: trace insert failed");
                    }
                }
            }
            PersistMsg::AuditJson(json) => {
                if let Ok(p) = serde_json::from_str::<GatewayAdminAuditPersistedJson>(&json)
                    && let Err(e) = conn.execute(
                        "INSERT OR REPLACE INTO audits (request_id, ts_ms, audit_json) VALUES (?1, ?2, ?3)",
                        params![p.request_id, p.timestamp_ms as i64, json],
                    )
                {
                    tracing::debug!(error = %e, request_id = %p.request_id, "admin sqlite: audit insert failed");
                }
            }
            PersistMsg::DeregisteredInstanceJson(json) => {
                if let Ok(p) = serde_json::from_str::<GatewayDeregisteredInstanceJson>(&json) {
                    if let Err(e) = conn.execute(
                        "INSERT INTO deregistered_instances (ts_ms, dcc_type, instance_id, reason, entry_json) VALUES (?1, ?2, ?3, ?4, ?5)",
                        params![
                            p.timestamp_ms.min(i64::MAX as u64) as i64,
                            p.dcc_type,
                            p.instance_id,
                            p.reason,
                            json,
                        ],
                    ) {
                        tracing::debug!(error = %e, "admin sqlite: deregistered instance insert failed");
                    } else {
                        prune_deregistered_instances(&mut conn, 100);
                    }
                }
            }
            PersistMsg::AddSkillPath(p) => {
                let now = SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .map(|d| d.as_millis() as i64)
                    .unwrap_or(0);
                if let Err(e) = conn.execute(
                    "INSERT OR IGNORE INTO skill_paths_custom (path, created_ms) VALUES (?1, ?2)",
                    params![p, now],
                ) {
                    tracing::debug!(error = %e, path = %p, "admin sqlite: skill path insert failed");
                }
            }
            PersistMsg::DeleteSkillPath(id) => {
                if let Err(e) = conn.execute("DELETE FROM skill_paths_custom WHERE id = ?1", params![id]) {
                    tracing::debug!(error = %e, id = id, "admin sqlite: skill path delete failed");
                }
            }
            PersistMsg::DeleteAgentMemory {
                id,
                layer,
                dcc_name,
                session_id,
                key_prefix,
            } => {
                if let Err(e) =
                    delete_agent_memory_rows(&mut conn, id, layer, dcc_name, session_id, key_prefix)
                {
                    tracing::debug!(error = %e, "admin sqlite: agent memory delete failed");
                }
            }
            PersistMsg::ToolCallEventJson(json) => {
                if let Ok(event) = serde_json::from_str::<dcc_mcp_models::ToolCallEvent>(&json)
                    && let Err(e) = conn.execute(
                        "INSERT OR REPLACE INTO tool_calls \
                         (request_id, session_id, parent_request_id, batch_id, tool_name, skill_name, \
                          dcc_type, instance_id, agent_id, transport, via_gateway, started_at_ms, \
                          duration_ms, success, error_message, error_kind, mcp_method, trace_id, span_id) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14, ?15, ?16, ?17, ?18, ?19)",
                        params![
                            event.request_id,
                            event.session_id,
                            event.parent_request_id,
                            event.batch_id,
                            event.tool_name,
                            event.skill_name,
                            event.dcc_type,
                            event.instance_id,
                            event.agent_id,
                            event.transport,
                            event.via_gateway.map(|v| v as i64),
                            event.started_at_ms,
                            event.duration_ms,
                            event.success as i64,
                            event.error_message,
                            event.error_kind,
                            event.mcp_method,
                            event.trace_id,
                            event.span_id,
                        ],
                    )
                {
                    tracing::debug!(error = %e, "admin sqlite: tool call event insert failed");
                }
            }
            PersistMsg::SessionUpsertJson(json) => {
                if let Ok(session) = serde_json::from_str::<dcc_mcp_models::Session>(&json) {
                    let end_reason_json = session
                        .end_reason
                        .as_ref()
                        .and_then(|r| serde_json::to_string(r).ok());
                    if let Err(e) = conn.execute(
                        "INSERT OR REPLACE INTO sessions \
                         (session_id, parent_session_id, dcc_type, instance_id, status, \
                          started_at_ms, last_activity_at_ms, ended_at_ms, end_reason_json, \
                          tool_call_count, error_count, core_version, adapter_version, build_sha) \
                         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11, ?12, ?13, ?14)",
                        params![
                            session.session_id,
                            session.parent_session_id,
                            session.dcc_type,
                            session.instance_id,
                            serde_json::to_string(&session.status).unwrap_or_default(),
                            session.started_at_ms,
                            session.last_activity_at_ms,
                            session.ended_at_ms,
                            end_reason_json,
                            session.tool_call_count as i64,
                            session.error_count as i64,
                            session.core_version,
                            session.adapter_version,
                            session.build_sha,
                        ],
                    ) {
                        tracing::debug!(error = %e, session_id = %session.session_id, "admin sqlite: session upsert failed");
                    }
                }
            }
            PersistMsg::FeedbackReportJson(json) => {
                if let Err(e) = insert_feedback_report(&conn, &json) {
                    tracing::debug!(error = %e, "admin sqlite: feedback report insert failed");
                }
            }
            PersistMsg::SessionEventJson(json) => {
                if let Ok(event) = serde_json::from_str::<serde_json::Value>(&json)
                    && let (Some(session_id), Some(event_type), Some(created_at_ms)) = (
                        event.get("session_id").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        event.get("event_type").and_then(|v| v.as_str()).map(|s| s.to_string()),
                        event.get("created_at_ms").and_then(|v| v.as_i64()),
                    )
                    && let Err(e) = conn.execute(
                        "INSERT INTO session_events (session_id, event_type, event_json, created_at_ms) \
                         VALUES (?1, ?2, ?3, ?4)",
                        params![session_id, event_type, json, created_at_ms],
                    )
                {
                    tracing::debug!(error = %e, "admin sqlite: session event insert failed");
                }
            }
            PersistMsg::ScriptPromotionBumpJson(json) => {
                if let Ok(bump) = serde_json::from_str::<ScriptPromotionBumpJson>(&json)
                    && let Err(e) = bump_script_promotion_counter(&conn, &bump)
                {
                    tracing::debug!(error = %e, "admin sqlite: script promotion bump failed");
                }
            }
        }
        n += 1;
        if n.is_multiple_of(128) {
            prune_old_rows(&mut conn, retention_days);
        }
    }
    let _ = conn.execute("PRAGMA optimize", []);
}

const FEEDBACK_REPORT_COLUMNS: &str = "id, repo, fingerprint, issues_url, route_rationale, dcc_type, \
    phase, severity, first_seen_ms, last_seen_ms, occurrence_count, report_json";

fn row_to_feedback_report(row: &rusqlite::Row<'_>) -> rusqlite::Result<FeedbackReportRow> {
    Ok(FeedbackReportRow {
        id: row.get(0)?,
        repo: row.get(1)?,
        fingerprint: row.get(2)?,
        issues_url: row.get(3)?,
        route_rationale: row.get(4)?,
        dcc_type: row.get(5)?,
        phase: row.get(6)?,
        severity: row.get(7)?,
        first_seen_ms: row.get(8)?,
        last_seen_ms: row.get(9)?,
        occurrence_count: row.get(10)?,
        report_json: row.get(11)?,
    })
}

/// Collapse one report onto its `(repo, fingerprint)` row, bumping the counter.
///
/// The `ON CONFLICT` clause is what makes ingest idempotent: the unique index
/// turns a repeat report into an `occurrence_count` increment and a
/// `last_seen_ms` refresh, leaving exactly one row per finding per repo.
fn upsert_feedback_report(
    conn: &mut Connection,
    row: &FeedbackReportInsert,
) -> Result<FeedbackReportRow, String> {
    let tx = conn.transaction().map_err(|e| e.to_string())?;
    tx.execute(
        "INSERT INTO feedback_reports \
         (repo, fingerprint, issues_url, route_rationale, dcc_type, phase, severity, \
          first_seen_ms, last_seen_ms, occurrence_count, report_json) \
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?8, 1, ?9) \
         ON CONFLICT (repo, fingerprint) DO UPDATE SET \
           last_seen_ms = excluded.last_seen_ms, \
           occurrence_count = occurrence_count + 1, \
           issues_url = COALESCE(excluded.issues_url, issues_url), \
           route_rationale = COALESCE(excluded.route_rationale, route_rationale), \
           dcc_type = excluded.dcc_type, \
           phase = excluded.phase, \
           severity = excluded.severity, \
           report_json = excluded.report_json",
        params![
            row.repo,
            row.fingerprint,
            row.issues_url,
            row.route_rationale,
            row.dcc_type,
            row.phase,
            row.severity,
            row.observed_at_ms,
            row.report_json,
        ],
    )
    .map_err(|e| e.to_string())?;
    let persisted = select_feedback_report(&tx, &row.repo, &row.fingerprint)?;
    tx.commit().map_err(|e| e.to_string())?;
    Ok(persisted)
}

fn select_feedback_report(
    conn: &Connection,
    repo: &str,
    fingerprint: &str,
) -> Result<FeedbackReportRow, String> {
    conn.query_row(
        &format!(
            "SELECT {FEEDBACK_REPORT_COLUMNS} FROM feedback_reports \
             WHERE repo = ?1 AND fingerprint = ?2"
        ),
        params![repo, fingerprint],
        row_to_feedback_report,
    )
    .map_err(|e| e.to_string())
}

fn prune_old_rows(conn: &mut Connection, retention_days: u32) {
    let days = u64::from(retention_days.clamp(1, 3650));
    let cutoff = SystemTime::now()
        .checked_sub(Duration::from_secs(days * 86_400))
        .and_then(|t| t.duration_since(UNIX_EPOCH).ok())
        .map(|d| d.as_millis() as i64)
        .unwrap_or(0);
    let _ = conn.execute("DELETE FROM traces WHERE started_ms < ?1", params![cutoff]);
    let _ = conn.execute("DELETE FROM audits WHERE ts_ms < ?1", params![cutoff]);
    let _ = conn.execute(
        "DELETE FROM sessions WHERE ended_at_ms IS NOT NULL AND ended_at_ms < ?1",
        params![cutoff],
    );
    let _ = conn.execute(
        "DELETE FROM session_events WHERE created_at_ms < ?1",
        params![cutoff],
    );
    let _ = conn.execute(
        "DELETE FROM tool_calls WHERE started_at_ms < ?1",
        params![cutoff],
    );
    let _ = prune_feedback_reports(conn, cutoff);
}

fn prune_deregistered_instances(conn: &mut Connection, keep: usize) {
    let keep = keep.max(1) as i64;
    let _ = conn.execute(
        "DELETE FROM deregistered_instances WHERE id NOT IN (
            SELECT id FROM deregistered_instances ORDER BY ts_ms DESC, id DESC LIMIT ?1
        )",
        params![keep],
    );
}

pub fn read_custom_skill_paths_for_startup(db_path: &Path) -> Vec<PathBuf> {
    let Ok(conn) = Connection::open_with_flags(
        db_path,
        rusqlite::OpenFlags::SQLITE_OPEN_READ_ONLY | rusqlite::OpenFlags::SQLITE_OPEN_NO_MUTEX,
    ) else {
        return Vec::new();
    };
    let mut stmt = match conn.prepare_cached("SELECT path FROM skill_paths_custom ORDER BY id ASC")
    {
        Ok(s) => s,
        Err(_) => return Vec::new(),
    };
    let rows = stmt.query_map([], |row| {
        let s: String = row.get(0)?;
        Ok(PathBuf::from(s))
    });
    let Ok(rows) = rows else {
        return Vec::new();
    };
    rows.filter_map(|r| r.ok()).collect()
}

fn delete_agent_memory_rows(
    conn: &mut Connection,
    id: Option<i64>,
    layer: Option<String>,
    dcc_name: Option<String>,
    session_id: Option<String>,
    key_prefix: Option<String>,
) -> rusqlite::Result<usize> {
    if let Some(id) = id {
        return conn.execute("DELETE FROM agent_memory WHERE id = ?1", params![id]);
    }
    let mut sql = String::from("DELETE FROM agent_memory WHERE 1 = 1");
    let mut values: Vec<Box<dyn ToSql>> = Vec::new();
    if let Some(value) = non_empty(layer.as_deref()) {
        sql.push_str(" AND layer = ?");
        values.push(Box::new(value.to_owned()));
    }
    if let Some(value) = non_empty(dcc_name.as_deref()) {
        sql.push_str(" AND dcc_name = ?");
        values.push(Box::new(value.to_owned()));
    }
    if let Some(value) = non_empty(session_id.as_deref()) {
        sql.push_str(" AND session_id = ?");
        values.push(Box::new(value.to_owned()));
    }
    if let Some(value) = non_empty(key_prefix.as_deref()) {
        sql.push_str(r" AND key LIKE ? ESCAPE '\'");
        values.push(Box::new(sqlite_like_prefix(value)));
    }
    let refs: Vec<&dyn ToSql> = values.iter().map(|value| value.as_ref()).collect();
    conn.execute(&sql, params_from_iter(refs))
}

fn non_empty(value: Option<&str>) -> Option<&str> {
    value.map(str::trim).filter(|value| !value.is_empty())
}

fn sqlite_like_prefix(value: &str) -> String {
    let mut out = String::with_capacity(value.len() + 1);
    for ch in value.chars() {
        if matches!(ch, '\\' | '%' | '_') {
            out.push('\\');
        }
        out.push(ch);
    }
    out.push('%');
    out
}

#[cfg(all(test, feature = "gateway-admin-sqlite"))]
mod tests {
    use super::*;
    use tempfile::tempdir;

    #[test]
    fn roundtrip_trace_json() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("t.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let json = r#"{"request_id":"r1","method":"tools/call","started_at":1700000000000,"total_ms":12,"ok":true,"spans":[]}"#;
        lane.try_persist_trace_json(json);
        drop(lane);
        let r = GatewayAdminSqliteReader::new(db);
        let list = r.list_traces_since_json(None, 10);
        assert!(list.iter().any(|s| s.contains("r1")));
    }

    #[test]
    fn roundtrip_audit_json() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("a.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let row = GatewayAdminAuditPersistedJson {
            timestamp_ms: 1_700_000_000_000,
            request_id: "rid".into(),
            trace_id: Some("trace-rid".into()),
            span_id: None,
            parent_span_id: None,
            method: Some("call".into()),
            instance_id: None,
            session_id: None,
            transport: Some("rest".into()),
            agent_id: Some("agent-1".into()),
            agent_name: Some("Test Agent".into()),
            agent_model: Some("gpt-test".into()),
            actor_id: Some("artist-1".into()),
            actor_name: Some("Layout Artist".into()),
            actor_email_hash: Some("sha256:actor".into()),
            client_platform: Some("custom-http".into()),
            client_os: Some("windows".into()),
            client_host: Some("workstation-7".into()),
            auth_subject: Some("user:artist-1".into()),
            source_ip: Some("192.0.2.44".into()),
            attribution_trust: Some(serde_json::json!({
                "actor_id": "self_reported",
                "auth_subject": "auth",
                "source_ip": "server_derived",
            })),
            parent_request_id: None,
            action: "x".into(),
            dcc_type: Some("maya".into()),
            success: true,
            error: None,
            duration_ms: Some(5),
            script_execution: Some(serde_json::json!({
                "sha256": "a".repeat(64),
                "reused": true,
                "reuse_key": "asset-builder",
            })),
            token_accounting: Some(serde_json::json!({
                "response_format": "toon",
                "saved_tokens": 12,
            })),
            llm_usage: None,
        };
        lane.try_persist_audit_json(&serde_json::to_string(&row).unwrap());
        drop(lane);
        let r = GatewayAdminSqliteReader::new(db);
        let list = r.list_audits_recent_json(10);
        assert_eq!(list.len(), 1);
        let back: GatewayAdminAuditPersistedJson = serde_json::from_str(&list[0]).unwrap();
        assert_eq!(back.request_id, "rid");
        assert_eq!(back.transport.as_deref(), Some("rest"));
        assert_eq!(back.agent_id.as_deref(), Some("agent-1"));
        assert_eq!(back.actor_id.as_deref(), Some("artist-1"));
        assert_eq!(back.client_platform.as_deref(), Some("custom-http"));
        assert_eq!(back.source_ip.as_deref(), Some("192.0.2.44"));
        assert_eq!(back.attribution_trust.unwrap()["auth_subject"], "auth");
        assert_eq!(back.script_execution.unwrap()["reuse_key"], "asset-builder");
        assert_eq!(back.token_accounting.unwrap()["saved_tokens"], 12);
    }

    #[test]
    fn roundtrip_skill_path_add_list_delete() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("sp.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        assert!(lane.try_add_skill_path("/tmp/skills/maya".to_string()));
        assert!(lane.try_add_skill_path("/tmp/skills/houdini".to_string()));
        // Wait for writer to process
        drop(lane);

        let r = GatewayAdminSqliteReader::new(db.clone());
        let paths = r.list_custom_skill_paths();
        assert_eq!(paths.len(), 2);
        assert!(paths.iter().any(|(_, p)| p == "/tmp/skills/maya"));
        assert!(paths.iter().any(|(_, p)| p == "/tmp/skills/houdini"));

        // Delete the first path
        let id_maya = paths
            .iter()
            .find(|(_, p)| p == "/tmp/skills/maya")
            .unwrap()
            .0;
        let lane2 = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        assert!(lane2.try_delete_skill_path(id_maya));
        drop(lane2);

        let r2 = GatewayAdminSqliteReader::new(db);
        let paths2 = r2.list_custom_skill_paths();
        assert_eq!(paths2.len(), 1);
        assert_eq!(paths2[0].1, "/tmp/skills/houdini");
    }

    #[test]
    fn roundtrip_deregistered_instance_json_keeps_latest_100() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("deregistered.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        for i in 0..105 {
            let row = GatewayDeregisteredInstanceJson {
                timestamp_ms: 1_700_000_000_000 + i,
                reason: "probe failure".into(),
                dcc_type: "maya".into(),
                instance_id: format!("instance-{i:03}"),
                entry: serde_json::json!({ "port": 18800 + i }),
            };
            lane.try_persist_deregistered_instance_json(&serde_json::to_string(&row).unwrap());
        }
        drop(lane);

        let rows = GatewayAdminSqliteReader::new(db).list_deregistered_instances_json(150);
        assert_eq!(rows.len(), 100);
        assert!(rows[0].contains("instance-104"));
        assert!(!rows.iter().any(|row| row.contains("instance-000")));
    }

    #[test]
    fn list_and_delete_agent_memory_rows() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("memory.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        drop(lane);

        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        let insert_memory = |key: &str, score: f64, payload: &str| {
            conn.execute(
                "INSERT INTO agent_memory \
                 (layer, key, session_id, dcc_name, score, created_unix_secs, payload_json) \
                 VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
                params!["longterm", key, "longterm", "maya", score, 10.0f64, payload],
            )
            .unwrap();
        };
        insert_memory(
            "pattern:tool_call:create_cube:ok",
            2.0,
            r#"{"tool_name":"create_cube","ok_count":2,"fail_count":0}"#,
        );
        insert_memory(
            "pattern:tool_call:maya_python__execute:fail",
            -1.0,
            r#"{"tool_name":"maya_python__execute","ok_count":0,"fail_count":1}"#,
        );
        insert_memory(
            "pattern:tool_call:mayaXpython__execute:fail",
            -1.0,
            r#"{"tool_name":"mayaXpython__execute","ok_count":0,"fail_count":1}"#,
        );
        insert_memory(
            "pattern:tool_call:maya%python__execute:ok",
            1.0,
            r#"{"tool_name":"maya%python__execute","ok_count":1,"fail_count":0}"#,
        );
        insert_memory(
            "pattern:tool_call:mayaZpython__execute:ok",
            1.0,
            r#"{"tool_name":"mayaZpython__execute","ok_count":1,"fail_count":0}"#,
        );
        drop(conn);

        let reader = GatewayAdminSqliteReader::new(db.clone());
        let rows = reader.list_agent_memory_json(
            Some("longterm"),
            Some("maya"),
            None,
            Some("pattern:"),
            10,
        );
        assert_eq!(rows.len(), 5);
        assert!(rows[0].contains("create_cube"));
        let rows = reader.list_agent_memory_json(
            Some("longterm"),
            Some("maya"),
            None,
            Some("pattern:tool_call:maya_python"),
            10,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("maya_python__execute"));
        let rows = reader.list_agent_memory_json(
            Some("longterm"),
            Some("maya"),
            None,
            Some("pattern:tool_call:maya%python"),
            10,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("maya%python__execute"));
        assert!(
            reader
                .list_agent_memory_json(None, None, Some("missing-session"), None, 10)
                .is_empty()
        );

        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        assert!(lane.try_delete_agent_memory(
            None,
            Some("longterm".into()),
            Some("maya".into()),
            None,
            Some("pattern:tool_call:maya_python".into()),
        ));
        assert!(lane.try_delete_agent_memory(
            None,
            Some("longterm".into()),
            Some("maya".into()),
            None,
            Some("pattern:tool_call:maya%python".into()),
        ));
        drop(lane);

        let rows =
            GatewayAdminSqliteReader::new(db).list_agent_memory_json(None, None, None, None, 10);
        assert_eq!(rows.len(), 3);
        assert!(rows.iter().any(|row| row.contains("create_cube")));
        assert!(rows.iter().any(|row| row.contains("mayaXpython__execute")));
        assert!(rows.iter().any(|row| row.contains("mayaZpython__execute")));
        assert!(!rows.iter().any(|row| row.contains("maya_python__execute")));
        assert!(!rows.iter().any(|row| row.contains("maya%python__execute")));
    }

    #[test]
    fn duplicate_path_insert_is_noop() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("dup.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        assert!(lane.try_add_skill_path("/tmp/dup".to_string()));
        assert!(lane.try_add_skill_path("/tmp/dup".to_string())); // INSERT OR IGNORE
        drop(lane);

        let r = GatewayAdminSqliteReader::new(db);
        let paths = r.list_custom_skill_paths();
        assert_eq!(paths.len(), 1);
    }

    #[test]
    fn read_custom_skill_paths_for_startup_works() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("startup.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        assert!(lane.try_add_skill_path("/opt/skills/blender".to_string()));
        drop(lane);

        let paths = read_custom_skill_paths_for_startup(&db);
        assert_eq!(paths.len(), 1);
        assert_eq!(paths[0], PathBuf::from("/opt/skills/blender"));
    }

    #[test]
    fn prune_old_rows_removes_expired() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("prune.sqlite");
        // Open and create schema
        let conn = Connection::open(&db).unwrap();
        conn.execute_batch(SCHEMA).unwrap();
        // Insert a trace with a very old timestamp (1 ms)
        conn.execute(
            "INSERT INTO traces (request_id, started_ms, trace_json) VALUES (?1, ?2, ?3)",
            params![
                "old-req",
                1i64,
                r#"{"request_id":"old-req","started_at":1}"#
            ],
        )
        .unwrap();
        // Insert a recent trace
        let now_ms = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_millis() as i64;
        conn.execute(
            "INSERT INTO traces (request_id, started_ms, trace_json) VALUES (?1, ?2, ?3)",
            params![
                "new-req",
                now_ms,
                r#"{"request_id":"new-req","started_at":0}"#
            ],
        )
        .unwrap();

        // Prune with retention = 1 day (old trace should be removed)
        let mut conn = conn;
        prune_old_rows(&mut conn, 1);

        let r = GatewayAdminSqliteReader::new(db);
        let traces = r.list_traces_since_json(None, 100);
        assert_eq!(traces.len(), 1);
        assert!(traces[0].contains("new-req"));
    }

    #[test]
    fn experiment_events_are_projected_from_the_existing_session_timeline() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("experiments.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        for event in [
            json!({
                "session_id": "maya-run-a",
                "event_type": "experiment.created",
                "created_at_ms": 10,
                "experiment_id": "exp-a",
                "name": "Maya scene validation"
            }),
            json!({
                "session_id": "maya-run-a",
                "event_type": "experiment.run.running",
                "created_at_ms": 20,
                "experiment_id": "exp-a",
                "run_id": "run-a"
            }),
            json!({
                "session_id": "maya-run-a",
                "event_type": "experiment.run.passed",
                "created_at_ms": 20,
                "experiment_id": "exp-a",
                "run_id": "run-a"
            }),
            json!({
                "session_id": "houdini-run-b",
                "event_type": "experiment.created",
                "created_at_ms": 30,
                "experiment_id": "exp-b",
                "name": "Houdini render validation"
            }),
        ] {
            lane.try_persist_session_event_json(&event.to_string());
        }
        drop(lane);

        let reader = GatewayAdminSqliteReader::new(db);
        let experiments = reader.list_experiments_json(10);
        assert_eq!(experiments.len(), 2);
        assert!(experiments[0].contains("exp-b"));

        let events = reader.list_experiment_events_json("exp-a", 10);
        assert_eq!(events.len(), 3);
        assert!(events.iter().all(|event| event.contains("exp-a")));
        assert!(events.last().unwrap().contains("experiment.run.passed"));
    }

    fn report(repo: &str, fingerprint: &str, seen_ms: i64) -> FeedbackReportInsert {
        FeedbackReportInsert {
            repo: repo.to_string(),
            fingerprint: fingerprint.to_string(),
            issues_url: Some(format!("https://github.com/{repo}/issues")),
            route_rationale: Some("adapter_phase".to_string()),
            dcc_type: "maya".to_string(),
            phase: "dispatch".to_string(),
            severity: "degraded".to_string(),
            observed_at_ms: seen_ms,
            report_json: format!("{{\"fingerprint\":\"{fingerprint}\"}}"),
        }
    }

    #[test]
    fn repeated_reports_collapse_into_one_row_with_a_counter() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "a".repeat(64));

        for seen_ms in [1_000_i64, 2_000, 3_000] {
            let row = lane
                .upsert_feedback_report(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, seen_ms))
                .expect("upsert");
            assert_eq!(row.occurrence_count, seen_ms / 1_000);
            assert_eq!(row.first_seen_ms, 1_000);
            assert_eq!(row.last_seen_ms, seen_ms);
            assert_eq!(row.repo, "dcc-mcp/dcc-mcp-maya");
        }

        let stored = lane.list_feedback_reports(10);
        assert_eq!(stored.len(), 1, "three reports must stay one row");
        assert_eq!(stored[0].occurrence_count, 3);
        assert_eq!(stored[0].last_seen_ms, 3_000);
        assert_eq!(
            stored[0].issues_url.as_deref(),
            Some("https://github.com/dcc-mcp/dcc-mcp-maya/issues")
        );
        assert_eq!(stored[0].route_rationale.as_deref(), Some("adapter_phase"));
    }

    #[test]
    fn upsert_keeps_the_row_id_stable_across_occurrences() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "b".repeat(64));

        let first = lane
            .upsert_feedback_report(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("first upsert");
        let second = lane
            .upsert_feedback_report(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 20))
            .expect("second upsert");

        assert_eq!(first.id, second.id, "a repeat report reuses the row id");
        assert_eq!(second.occurrence_count, 2);
    }

    #[test]
    fn the_same_fingerprint_in_two_repos_stays_two_rows() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "c".repeat(64));

        lane.upsert_feedback_report(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("maya upsert");
        lane.upsert_feedback_report(&report("dcc-mcp/dcc-mcp-core", &fingerprint, 10))
            .expect("core upsert");

        let stored = lane.list_feedback_reports(10);
        assert_eq!(stored.len(), 2, "the dedup key is (repo, fingerprint)");
    }

    #[test]
    fn unrouted_reports_dedup_under_the_empty_repo() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "d".repeat(64));
        let unrouted = FeedbackReportInsert {
            repo: String::new(),
            issues_url: None,
            route_rationale: None,
            ..report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10)
        };

        lane.upsert_feedback_report(&unrouted).expect("first");
        let row = lane.upsert_feedback_report(&unrouted).expect("second");

        assert_eq!(row.occurrence_count, 2);
        assert_eq!(lane.list_feedback_reports(10).len(), 1);
    }

    #[test]
    fn lookup_by_dedup_key_finds_the_collapsed_row() {
        let dir = tempdir().unwrap();
        let db = dir.path().join("f.sqlite");
        let lane = GatewayAdminSqliteLane::spawn(db.clone(), 30).expect("spawn");
        let fingerprint = format!("sha256:{}", "e".repeat(64));
        lane.upsert_feedback_report(&report("dcc-mcp/dcc-mcp-maya", &fingerprint, 10))
            .expect("upsert");

        let found = lane
            .get_feedback_report("dcc-mcp/dcc-mcp-maya", &fingerprint)
            .expect("row exists");
        assert_eq!(found.occurrence_count, 1);
        assert!(
            lane.get_feedback_report("dcc-mcp/other", &fingerprint)
                .is_none()
        );
    }
}
