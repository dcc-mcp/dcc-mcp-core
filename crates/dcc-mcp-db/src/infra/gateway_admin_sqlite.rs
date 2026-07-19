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

use crate::domain::gateway_admin_audit::GatewayAdminAuditPersistedJson;
use crate::domain::gateway_admin_deregistered::GatewayDeregisteredInstanceJson;
use crate::infra::gateway_admin_schema::GATEWAY_ADMIN_SQLITE_DDL;

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

    pub fn list_agent_memory_json(
        &self,
        layer: Option<&str>,
        dcc_name: Option<&str>,
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
        }
        n += 1;
        if n.is_multiple_of(128) {
            prune_old_rows(&mut conn, retention_days);
        }
    }
    let _ = conn.execute("PRAGMA optimize", []);
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
        let rows =
            reader.list_agent_memory_json(Some("longterm"), Some("maya"), Some("pattern:"), 10);
        assert_eq!(rows.len(), 5);
        assert!(rows[0].contains("create_cube"));
        let rows = reader.list_agent_memory_json(
            Some("longterm"),
            Some("maya"),
            Some("pattern:tool_call:maya_python"),
            10,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("maya_python__execute"));
        let rows = reader.list_agent_memory_json(
            Some("longterm"),
            Some("maya"),
            Some("pattern:tool_call:maya%python"),
            10,
        );
        assert_eq!(rows.len(), 1);
        assert!(rows[0].contains("maya%python__execute"));

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

        let rows = GatewayAdminSqliteReader::new(db).list_agent_memory_json(None, None, None, 10);
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
}
