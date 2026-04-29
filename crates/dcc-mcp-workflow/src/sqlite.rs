//! SQLite persistence for workflow runs (issue #348 execution path).
//!
//! Wires a write-through [`WorkflowStorage`] that the executor calls on every
//! workflow / step transition. On startup, [`WorkflowStorage::recover`]
//! flips any non-terminal rows to `interrupted` so a restarted server can
//! surface the interruption on `$/dcc.workflowUpdated`.

use std::path::Path;
use std::sync::Arc;

use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};
use serde_json::Value;
use tracing::warn;
use uuid::Uuid;

use crate::idempotency::IdempotencyStore;
use crate::policy::IdempotencyScope;
use crate::spec::{WorkflowSpec, WorkflowStatus};

/// Full DDL for the workflow persistence schema. Idempotent.
pub const MIGRATION_V1: &str = r#"
CREATE TABLE IF NOT EXISTS workflows (
    id              TEXT PRIMARY KEY NOT NULL,
    root_job_id     TEXT NOT NULL,
    name            TEXT NOT NULL,
    status          TEXT NOT NULL,
    spec_json       TEXT NOT NULL,
    inputs_json     TEXT NOT NULL DEFAULT '{}',
    step_outputs_json TEXT NOT NULL DEFAULT '{}',
    current_step_id TEXT,
    started_at      INTEGER,
    completed_at    INTEGER,
    created_at      INTEGER NOT NULL DEFAULT (strftime('%s', 'now'))
);

CREATE INDEX IF NOT EXISTS workflows_status_idx ON workflows(status);

CREATE TABLE IF NOT EXISTS workflow_steps (
    workflow_id     TEXT NOT NULL REFERENCES workflows(id) ON DELETE CASCADE,
    step_id         TEXT NOT NULL,
    status          TEXT NOT NULL,
    result_json     TEXT,
    error           TEXT,
    started_at      INTEGER,
    completed_at    INTEGER,
    PRIMARY KEY (workflow_id, step_id)
);

CREATE INDEX IF NOT EXISTS workflow_steps_status_idx ON workflow_steps(status);

-- Persistent idempotency cache (issue #566). One row per
-- (scope, workflow_id, rendered_key) tuple; `workflow_id = ''` is the
-- sentinel for the global scope (so the composite primary key works
-- without nullable columns). Rows in the workflow scope are wiped via
-- the AFTER DELETE trigger below when their owning workflow row is
-- removed; global rows live until `expires_at` is reached or until an
-- admin tool purges them explicitly.
CREATE TABLE IF NOT EXISTS workflow_idempotency (
    scope            TEXT    NOT NULL CHECK (scope IN ('workflow', 'global')),
    workflow_id      TEXT    NOT NULL,
    rendered_key     TEXT    NOT NULL,
    step_id          TEXT    NOT NULL,
    result_json      TEXT    NOT NULL,
    created_at       INTEGER NOT NULL DEFAULT (strftime('%s', 'now')),
    expires_at       INTEGER NULL,
    PRIMARY KEY (scope, workflow_id, rendered_key)
);

CREATE INDEX IF NOT EXISTS workflow_idempotency_expires_idx
    ON workflow_idempotency(expires_at)
    WHERE expires_at IS NOT NULL;

CREATE TRIGGER IF NOT EXISTS workflow_idempotency_cascade_on_workflow_delete
    AFTER DELETE ON workflows
    BEGIN
        DELETE FROM workflow_idempotency
            WHERE scope = 'workflow' AND workflow_id = OLD.id;
    END;
"#;

/// Apply the schema. Idempotent.
pub fn apply_migrations(conn: &Connection) -> rusqlite::Result<()> {
    conn.execute_batch(MIGRATION_V1)
}

/// A row in the `workflows` table.
#[derive(Debug, Clone)]
pub struct WorkflowRow {
    /// Workflow UUID (primary key).
    pub id: Uuid,
    /// Root job UUID.
    pub root_job_id: Uuid,
    /// Spec `name` field.
    pub name: String,
    /// Current status.
    pub status: WorkflowStatus,
    /// Current step id, if any.
    pub current_step_id: Option<String>,
}

/// SQLite-backed workflow state writer.
#[derive(Debug)]
pub struct WorkflowStorage {
    conn: Mutex<Connection>,
}

/// Errors returned by [`WorkflowStorage`].
#[derive(Debug, thiserror::Error)]
pub enum WorkflowStorageError {
    /// Wrapped `rusqlite` error.
    #[error("sqlite error: {0}")]
    Sqlite(#[from] rusqlite::Error),
    /// Wrapped `serde_json` error.
    #[error("json error: {0}")]
    Json(#[from] serde_json::Error),
}

impl WorkflowStorage {
    /// Open (or create) a database at `path`. Parent dirs are created.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, WorkflowStorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                let _ = std::fs::create_dir_all(parent);
            }
        }
        let conn = Connection::open(path)?;
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;",
        )?;
        apply_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database. Used in tests.
    pub fn open_in_memory() -> Result<Self, WorkflowStorageError> {
        let conn = Connection::open_in_memory()?;
        apply_migrations(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Insert a new workflow row.
    pub fn insert_workflow(
        &self,
        id: Uuid,
        root_job_id: Uuid,
        spec: &WorkflowSpec,
        inputs: &serde_json::Value,
    ) -> Result<(), WorkflowStorageError> {
        let spec_json = serde_json::to_string(spec)?;
        let inputs_json = serde_json::to_string(inputs)?;
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO workflows
                (id, root_job_id, name, status, spec_json, inputs_json, step_outputs_json, current_step_id, started_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, '{}', NULL, strftime('%s','now'))",
            params![
                id.to_string(),
                root_job_id.to_string(),
                spec.name,
                WorkflowStatus::Pending.as_str(),
                spec_json,
                inputs_json,
            ],
        )?;
        Ok(())
    }

    /// Update workflow-level status + current step id.
    pub fn update_workflow_status(
        &self,
        id: Uuid,
        status: WorkflowStatus,
        current_step_id: Option<&str>,
    ) -> Result<(), WorkflowStorageError> {
        let conn = self.conn.lock();
        let completed_sql = if status.is_terminal() {
            ", completed_at = strftime('%s','now')"
        } else {
            ""
        };
        let sql = format!(
            "UPDATE workflows SET status = ?1, current_step_id = ?2 {completed_sql} WHERE id = ?3"
        );
        conn.execute(
            &sql,
            params![status.as_str(), current_step_id, id.to_string()],
        )?;
        Ok(())
    }

    /// Update aggregated step outputs JSON blob.
    pub fn update_step_outputs(
        &self,
        id: Uuid,
        step_outputs: &serde_json::Value,
    ) -> Result<(), WorkflowStorageError> {
        let outputs_json = serde_json::to_string(step_outputs)?;
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE workflows SET step_outputs_json = ?1 WHERE id = ?2",
            params![outputs_json, id.to_string()],
        )?;
        Ok(())
    }

    /// Record (or replace) a single step row.
    pub fn upsert_step(
        &self,
        workflow_id: Uuid,
        step_id: &str,
        status: &str,
        result: Option<&serde_json::Value>,
        error: Option<&str>,
    ) -> Result<(), WorkflowStorageError> {
        let result_json = result.map(serde_json::to_string).transpose()?;
        let conn = self.conn.lock();
        let terminal = matches!(status, "completed" | "failed" | "cancelled" | "interrupted");
        let started_sql = if matches!(status, "running") {
            "strftime('%s','now')"
        } else {
            "COALESCE((SELECT started_at FROM workflow_steps WHERE workflow_id=?1 AND step_id=?2), NULL)"
        };
        let completed_sql = if terminal {
            "strftime('%s','now')"
        } else {
            "NULL"
        };
        let sql = format!(
            "INSERT OR REPLACE INTO workflow_steps
                (workflow_id, step_id, status, result_json, error, started_at, completed_at)
             VALUES (?1, ?2, ?3, ?4, ?5, {started_sql}, {completed_sql})"
        );
        conn.execute(
            &sql,
            params![workflow_id.to_string(), step_id, status, result_json, error],
        )?;
        Ok(())
    }

    /// Fetch all non-terminal workflow rows. Used by recovery.
    pub fn list_non_terminal(&self) -> Result<Vec<WorkflowRow>, WorkflowStorageError> {
        let conn = self.conn.lock();
        let mut stmt = conn.prepare(
            "SELECT id, root_job_id, name, status, current_step_id \
             FROM workflows WHERE status IN ('pending','running')",
        )?;
        let rows = stmt
            .query_map([], |r| {
                Ok((
                    r.get::<_, String>(0)?,
                    r.get::<_, String>(1)?,
                    r.get::<_, String>(2)?,
                    r.get::<_, String>(3)?,
                    r.get::<_, Option<String>>(4)?,
                ))
            })?
            .collect::<rusqlite::Result<Vec<_>>>()?;
        let mut out = Vec::with_capacity(rows.len());
        for (id, root_job_id, name, status, current_step_id) in rows {
            let wid = Uuid::parse_str(&id).unwrap_or_else(|_| Uuid::nil());
            let rjid = Uuid::parse_str(&root_job_id).unwrap_or_else(|_| Uuid::nil());
            let s = parse_status(&status);
            out.push(WorkflowRow {
                id: wid,
                root_job_id: rjid,
                name,
                status: s,
                current_step_id,
            });
        }
        Ok(out)
    }

    /// Flip every non-terminal workflow row to `interrupted`. Returns the
    /// rows that were flipped (so the caller can emit one last
    /// `$/dcc.workflowUpdated` per).
    pub fn recover(&self) -> Result<Vec<WorkflowRow>, WorkflowStorageError> {
        let rows = self.list_non_terminal()?;
        for row in &rows {
            self.update_workflow_status(
                row.id,
                WorkflowStatus::Interrupted,
                row.current_step_id.as_deref(),
            )?;
            if let Some(ref step_id) = row.current_step_id {
                let _ =
                    self.upsert_step(row.id, step_id, "interrupted", None, Some("server restart"));
            }
        }
        Ok(rows)
    }

    // ── Idempotency cache helpers (issue #566) ──────────────────────────

    /// Look up a non-expired idempotency cache entry. Returns the parsed
    /// `result_json` payload.
    pub fn idem_get(
        &self,
        scope: IdempotencyScope,
        workflow_id: Uuid,
        rendered_key: &str,
    ) -> Result<Option<Value>, WorkflowStorageError> {
        let conn = self.conn.lock();
        let scope_str = scope.as_str();
        let wid_str = idem_workflow_key(scope, workflow_id);
        let now = now_unix_secs();
        let row: Option<String> = conn
            .query_row(
                "SELECT result_json FROM workflow_idempotency \
                 WHERE scope = ?1 AND workflow_id = ?2 AND rendered_key = ?3 \
                 AND (expires_at IS NULL OR expires_at > ?4)",
                params![scope_str, wid_str, rendered_key, now],
                |r| r.get(0),
            )
            .optional()?;
        match row {
            Some(s) => Ok(Some(serde_json::from_str(&s)?)),
            None => Ok(None),
        }
    }

    /// Insert (or replace) an idempotency cache entry. `ttl_secs = None`
    /// (or `Some(0)`) means the row never expires automatically.
    pub fn idem_put(
        &self,
        scope: IdempotencyScope,
        workflow_id: Uuid,
        rendered_key: &str,
        step_id: &str,
        result: &Value,
        ttl_secs: Option<u64>,
    ) -> Result<(), WorkflowStorageError> {
        let result_json = serde_json::to_string(result)?;
        let now = now_unix_secs();
        let expires_at: Option<i64> = ttl_secs
            .filter(|n| *n > 0)
            .and_then(|n| i64::try_from(n).ok())
            .and_then(|n| now.checked_add(n));
        let conn = self.conn.lock();
        conn.execute(
            "INSERT OR REPLACE INTO workflow_idempotency \
                (scope, workflow_id, rendered_key, step_id, result_json, created_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params![
                scope.as_str(),
                idem_workflow_key(scope, workflow_id),
                rendered_key,
                step_id,
                result_json,
                now,
                expires_at,
            ],
        )?;
        Ok(())
    }

    /// Remove every cache row whose `expires_at` is at or before now.
    /// Returns the number of rows deleted.
    pub fn idem_purge_expired(&self) -> Result<usize, WorkflowStorageError> {
        let conn = self.conn.lock();
        let n = conn.execute(
            "DELETE FROM workflow_idempotency \
             WHERE expires_at IS NOT NULL AND expires_at <= ?1",
            params![now_unix_secs()],
        )?;
        Ok(n)
    }

    /// Total number of idempotency rows. Testing helper.
    pub fn idem_len(&self) -> Result<usize, WorkflowStorageError> {
        let conn = self.conn.lock();
        let n: i64 = conn.query_row("SELECT COUNT(*) FROM workflow_idempotency", [], |r| {
            r.get(0)
        })?;
        Ok(n as usize)
    }

    // ── Resume helpers (issue #565) ────────────────────────────────────

    /// Fetch every artefact needed by [`crate::WorkflowExecutor::resume`]:
    /// the persisted spec, the original inputs, and the per-step status
    /// + cached outputs. Returns `None` if the workflow id is unknown.
    pub fn load_resume_snapshot(
        &self,
        workflow_id: Uuid,
    ) -> Result<Option<ResumeSnapshot>, WorkflowStorageError> {
        let conn = self.conn.lock();
        let row: Option<(String, String, String, String)> = conn
            .query_row(
                "SELECT status, spec_json, inputs_json, step_outputs_json \
                 FROM workflows WHERE id = ?1",
                params![workflow_id.to_string()],
                |r| Ok((r.get(0)?, r.get(1)?, r.get(2)?, r.get(3)?)),
            )
            .optional()?;
        let Some((status_str, spec_json, inputs_json, outputs_json)) = row else {
            return Ok(None);
        };
        let mut stmt = conn.prepare(
            "SELECT step_id, status, result_json FROM workflow_steps \
             WHERE workflow_id = ?1",
        )?;
        let mut completed: Vec<(String, Value)> = Vec::new();
        let rows = stmt.query_map(params![workflow_id.to_string()], |r| {
            let id: String = r.get(0)?;
            let status: String = r.get(1)?;
            let out: Option<String> = r.get(2)?;
            Ok((id, status, out))
        })?;
        for r in rows {
            let (id, status, out) = r?;
            if status == "completed" {
                let parsed = match out {
                    Some(s) => serde_json::from_str(&s)?,
                    None => Value::Null,
                };
                completed.push((id, parsed));
            }
        }
        Ok(Some(ResumeSnapshot {
            status: parse_status(&status_str),
            spec_json,
            inputs_json,
            outputs_json,
            completed_steps: completed,
        }))
    }

    /// Reset a workflow row's status back to `Pending` so a resume run
    /// can drive it forward again. Clears `current_step_id` and
    /// `error_msg`. Does NOT touch `spec_json`, `inputs_json`, or
    /// existing `workflow_steps` rows.
    pub fn reset_for_resume(&self, workflow_id: Uuid) -> Result<(), WorkflowStorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE workflows SET status = 'pending', current_step_id = NULL, \
                 error_msg = NULL, started_at = strftime('%s', 'now') \
             WHERE id = ?1",
            params![workflow_id.to_string()],
        )?;
        Ok(())
    }
}

/// All persisted state needed to plan a resume — see
/// [`WorkflowStorage::load_resume_snapshot`].
#[derive(Debug, Clone)]
pub struct ResumeSnapshot {
    /// Last-persisted workflow status.
    pub status: WorkflowStatus,
    /// Original spec serialised at first run.
    pub spec_json: String,
    /// Original inputs serialised at first run.
    pub inputs_json: String,
    /// Latest persisted step-output bag (keyed by step id at the
    /// outer-spec level — used for context restoration).
    pub outputs_json: String,
    /// `(step_id, output)` for every step that reached `completed`.
    pub completed_steps: Vec<(String, Value)>,
}

/// Compute the canonical SHA-256 hex digest of a workflow spec. Two
/// specs that hash the same will produce identical executor behaviour;
/// adapters can compare this hash across catalog reloads to detect
/// drift before issuing `workflows.resume`.
#[must_use]
pub fn compute_spec_hash(spec: &WorkflowSpec) -> String {
    use sha2::{Digest, Sha256};
    use std::fmt::Write;
    let canonical = serde_json::to_string(spec).expect("WorkflowSpec serialises");
    let mut hasher = Sha256::new();
    hasher.update(canonical.as_bytes());
    let bytes = hasher.finalize();
    let mut out = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        write!(out, "{b:02x}").expect("writes to String never fail");
    }
    out
}

/// Compose the `workflow_id` column value for a cache row. Global rows
/// use the empty-string sentinel so the composite primary key works
/// without a nullable column.
fn idem_workflow_key(scope: IdempotencyScope, workflow_id: Uuid) -> String {
    match scope {
        IdempotencyScope::Global => String::new(),
        IdempotencyScope::Workflow => workflow_id.to_string(),
    }
}

fn now_unix_secs() -> i64 {
    use std::time::{SystemTime, UNIX_EPOCH};
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

/// SQLite-backed [`IdempotencyStore`]. Wraps an `Arc<WorkflowStorage>`
/// so it can share a connection pool with the workflow row writer.
#[derive(Debug, Clone)]
pub struct SqliteIdempotencyStore {
    storage: Arc<WorkflowStorage>,
}

impl SqliteIdempotencyStore {
    /// Wrap a [`WorkflowStorage`] handle.
    pub fn new(storage: Arc<WorkflowStorage>) -> Self {
        Self { storage }
    }
}

impl IdempotencyStore for SqliteIdempotencyStore {
    fn get(&self, scope: IdempotencyScope, workflow_id: Uuid, key: &str) -> Option<Value> {
        match self.storage.idem_get(scope, workflow_id, key) {
            Ok(v) => v,
            Err(e) => {
                warn!(error = %e, "SqliteIdempotencyStore::get failed");
                None
            }
        }
    }

    fn put(
        &self,
        scope: IdempotencyScope,
        workflow_id: Uuid,
        key: &str,
        step_id: &str,
        output: Value,
        ttl_secs: Option<u64>,
    ) {
        if let Err(e) = self
            .storage
            .idem_put(scope, workflow_id, key, step_id, &output, ttl_secs)
        {
            warn!(error = %e, "SqliteIdempotencyStore::put failed");
        }
    }

    fn purge_expired(&self) -> usize {
        self.storage.idem_purge_expired().unwrap_or_else(|e| {
            warn!(error = %e, "SqliteIdempotencyStore::purge_expired failed");
            0
        })
    }
}

fn parse_status(s: &str) -> WorkflowStatus {
    match s {
        "pending" => WorkflowStatus::Pending,
        "running" => WorkflowStatus::Running,
        "completed" => WorkflowStatus::Completed,
        "failed" => WorkflowStatus::Failed,
        "cancelled" => WorkflowStatus::Cancelled,
        "interrupted" => WorkflowStatus::Interrupted,
        _ => WorkflowStatus::Pending,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_spec() -> WorkflowSpec {
        WorkflowSpec::from_yaml("name: demo\nsteps:\n  - id: s1\n    tool: scene.get_info\n")
            .unwrap()
    }

    #[test]
    fn migration_applies_on_fresh_db() {
        let conn = Connection::open_in_memory().unwrap();
        apply_migrations(&conn).unwrap();
        apply_migrations(&conn).unwrap();
    }

    #[test]
    fn insert_and_list_and_recover() {
        let st = WorkflowStorage::open_in_memory().unwrap();
        let id = Uuid::new_v4();
        let rjid = Uuid::new_v4();
        let spec = sample_spec();
        st.insert_workflow(id, rjid, &spec, &serde_json::json!({"k": "v"}))
            .unwrap();
        st.update_workflow_status(id, WorkflowStatus::Running, Some("s1"))
            .unwrap();

        let rows = st.list_non_terminal().unwrap();
        assert_eq!(rows.len(), 1);
        assert_eq!(rows[0].status, WorkflowStatus::Running);

        let flipped = st.recover().unwrap();
        assert_eq!(flipped.len(), 1);
        let rows_after = st.list_non_terminal().unwrap();
        assert!(
            rows_after.is_empty(),
            "recover must clear non-terminal rows"
        );
    }

    #[test]
    fn upsert_step_rows() {
        let st = WorkflowStorage::open_in_memory().unwrap();
        let id = Uuid::new_v4();
        st.insert_workflow(id, Uuid::new_v4(), &sample_spec(), &serde_json::json!({}))
            .unwrap();
        st.upsert_step(id, "s1", "running", None, None).unwrap();
        st.upsert_step(
            id,
            "s1",
            "completed",
            Some(&serde_json::json!({"ok": true})),
            None,
        )
        .unwrap();
        let conn = st.conn.lock();
        let status: String = conn
            .query_row(
                "SELECT status FROM workflow_steps WHERE workflow_id=?1 AND step_id=?2",
                params![id.to_string(), "s1"],
                |r| r.get(0),
            )
            .unwrap();
        assert_eq!(status, "completed");
    }

    // ── Idempotency cache (issue #566) ──────────────────────────────────

    fn fresh_storage_with_workflow(id: Uuid) -> Arc<WorkflowStorage> {
        let st = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        st.insert_workflow(id, Uuid::new_v4(), &sample_spec(), &serde_json::json!({}))
            .unwrap();
        st
    }

    #[test]
    fn idem_get_returns_none_when_missing() {
        let st = WorkflowStorage::open_in_memory().unwrap();
        let v = st
            .idem_get(IdempotencyScope::Workflow, Uuid::new_v4(), "k")
            .unwrap();
        assert!(v.is_none());
    }

    #[test]
    fn idem_put_then_get_round_trips_workflow_scope() {
        let id = Uuid::new_v4();
        let st = fresh_storage_with_workflow(id);
        let store = SqliteIdempotencyStore::new(Arc::clone(&st));
        store.put(
            IdempotencyScope::Workflow,
            id,
            "k1",
            "step",
            serde_json::json!({"v": 1}),
            None,
        );
        assert_eq!(
            store.get(IdempotencyScope::Workflow, id, "k1"),
            Some(serde_json::json!({"v": 1}))
        );
    }

    #[test]
    fn idem_workflow_scope_isolates_by_id() {
        let st = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        st.insert_workflow(w1, Uuid::new_v4(), &sample_spec(), &serde_json::json!({}))
            .unwrap();
        st.insert_workflow(w2, Uuid::new_v4(), &sample_spec(), &serde_json::json!({}))
            .unwrap();
        let store = SqliteIdempotencyStore::new(st);
        store.put(
            IdempotencyScope::Workflow,
            w1,
            "k",
            "s",
            serde_json::json!(1),
            None,
        );
        assert!(store.get(IdempotencyScope::Workflow, w2, "k").is_none());
    }

    #[test]
    fn idem_global_scope_crosses_workflows() {
        let st = Arc::new(WorkflowStorage::open_in_memory().unwrap());
        let store = SqliteIdempotencyStore::new(Arc::clone(&st));
        let w1 = Uuid::new_v4();
        let w2 = Uuid::new_v4();
        store.put(
            IdempotencyScope::Global,
            w1,
            "shared",
            "s",
            serde_json::json!("hello"),
            None,
        );
        assert_eq!(
            store.get(IdempotencyScope::Global, w2, "shared"),
            Some(serde_json::json!("hello")),
            "global scope must be reachable from any workflow id"
        );
    }

    #[test]
    fn idem_ttl_is_honoured_via_now_filter() {
        let id = Uuid::new_v4();
        let st = fresh_storage_with_workflow(id);
        // Forge a row whose `expires_at` is already in the past — avoids
        // sleeping in tests.
        let conn = st.conn.lock();
        let past = now_unix_secs() - 10;
        conn.execute(
            "INSERT INTO workflow_idempotency \
                (scope, workflow_id, rendered_key, step_id, result_json, created_at, expires_at) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
            params!["workflow", id.to_string(), "old", "s", "\"v\"", past, past],
        )
        .unwrap();
        drop(conn);
        let store = SqliteIdempotencyStore::new(Arc::clone(&st));
        assert!(store.get(IdempotencyScope::Workflow, id, "old").is_none());
        assert_eq!(store.purge_expired(), 1);
        assert_eq!(st.idem_len().unwrap(), 0);
    }

    #[test]
    fn idem_cascade_trigger_drops_workflow_scope_rows_on_workflow_delete() {
        let id = Uuid::new_v4();
        let st = fresh_storage_with_workflow(id);
        let store = SqliteIdempotencyStore::new(Arc::clone(&st));
        store.put(
            IdempotencyScope::Workflow,
            id,
            "k",
            "s",
            serde_json::json!(1),
            None,
        );
        store.put(
            IdempotencyScope::Global,
            id,
            "g",
            "s",
            serde_json::json!(2),
            None,
        );
        assert_eq!(st.idem_len().unwrap(), 2);
        let conn = st.conn.lock();
        conn.execute(
            "DELETE FROM workflows WHERE id = ?1",
            params![id.to_string()],
        )
        .unwrap();
        drop(conn);
        // Only the workflow-scoped row should be gone; global survives.
        assert_eq!(st.idem_len().unwrap(), 1);
        assert!(store.get(IdempotencyScope::Workflow, id, "k").is_none());
        assert_eq!(
            store.get(IdempotencyScope::Global, id, "g"),
            Some(serde_json::json!(2))
        );
    }

    #[test]
    fn idem_persists_across_store_instances_on_same_db() {
        // Round-trip across two SqliteIdempotencyStore handles backed by
        // the same WorkflowStorage — proves the cache survives a logical
        // "restart" within the same DB file.
        let id = Uuid::new_v4();
        let st = fresh_storage_with_workflow(id);
        let store_a = SqliteIdempotencyStore::new(Arc::clone(&st));
        store_a.put(
            IdempotencyScope::Workflow,
            id,
            "k",
            "s",
            serde_json::json!({"export": "ok"}),
            None,
        );
        // Pretend the executor was rebuilt — the data is keyed off the
        // shared WorkflowStorage, not the in-memory store struct.
        let store_b = SqliteIdempotencyStore::new(st);
        assert_eq!(
            store_b.get(IdempotencyScope::Workflow, id, "k"),
            Some(serde_json::json!({"export": "ok"})),
        );
    }
}
