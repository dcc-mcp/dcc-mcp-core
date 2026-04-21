//! SQLite persistence for workflow runs (issue #348 execution path).
//!
//! Wires a write-through [`WorkflowStorage`] that the executor calls on every
//! workflow / step transition. On startup, [`WorkflowStorage::recover`]
//! flips any non-terminal rows to `interrupted` so a restarted server can
//! surface the interruption on `$/dcc.workflowUpdated`.

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{Connection, params};
use uuid::Uuid;

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
}
