//! SQLite [`JobStorage`] implementation — issue #328.
//!
//! Gated behind the `job-persist-sqlite` Cargo feature so the default
//! build has zero SQLite cost. Uses the bundled `rusqlite` driver so
//! downstream wheels do not need a system libsqlite.
//!
//! Synchronous write-through — every mutation runs a parameterised
//! `INSERT OR REPLACE` / `UPDATE` under a short `Mutex`. The connection
//! is not shared across threads directly (rusqlite `Connection` is not
//! `Sync`), so we serialise writes behind `parking_lot::Mutex` which is
//! the same locking strategy used elsewhere in the workspace.

use std::path::Path;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use rusqlite::{Connection, OptionalExtension, params};

use crate::job::{Job, JobProgress, JobStatus};
use crate::job_storage::{JobFilter, JobStorage, JobStorageError, PersistedJob};

const SCHEMA: &str = r#"
CREATE TABLE IF NOT EXISTS jobs (
    job_id TEXT PRIMARY KEY,
    parent_job_id TEXT,
    tool TEXT NOT NULL,
    status TEXT NOT NULL,
    progress_json TEXT,
    created_at TEXT NOT NULL,
    updated_at TEXT NOT NULL,
    error TEXT,
    result_json TEXT
);
CREATE INDEX IF NOT EXISTS jobs_status_idx ON jobs(status);
CREATE INDEX IF NOT EXISTS jobs_parent_idx ON jobs(parent_job_id);
CREATE INDEX IF NOT EXISTS jobs_updated_idx ON jobs(updated_at);
"#;

/// SQLite-backed [`JobStorage`].
#[derive(Debug)]
pub struct SqliteStorage {
    conn: Mutex<Connection>,
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at `path` and run schema
    /// migrations. Parent directories are created as needed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JobStorageError> {
        let path = path.as_ref();
        if let Some(parent) = path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                JobStorageError::Backend(format!(
                    "failed to create parent directory for job storage: {e}"
                ))
            })?;
        }
        let conn = Connection::open(path).map_err(map_err)?;
        // Pragmas: WAL for concurrent reads, NORMAL sync is a good
        // durability/perf tradeoff for a side-car status store.
        conn.execute_batch(
            "PRAGMA journal_mode=WAL; \
             PRAGMA synchronous=NORMAL; \
             PRAGMA foreign_keys=ON;",
        )
        .map_err(map_err)?;
        conn.execute_batch(SCHEMA).map_err(map_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }

    /// Open an in-memory database — used by tests that want the real
    /// SQLite code path without touching the filesystem.
    pub fn open_in_memory() -> Result<Self, JobStorageError> {
        let conn = Connection::open_in_memory().map_err(map_err)?;
        conn.execute_batch(SCHEMA).map_err(map_err)?;
        Ok(Self {
            conn: Mutex::new(conn),
        })
    }
}

impl JobStorage for SqliteStorage {
    fn put(&self, job: &Job) -> Result<(), JobStorageError> {
        let row = PersistedJob::from_job(job);
        let progress_json = row
            .progress
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| JobStorageError::Decode(e.to_string()))?;
        let result_json = row
            .result
            .as_ref()
            .map(serde_json::to_string)
            .transpose()
            .map_err(|e| JobStorageError::Decode(e.to_string()))?;
        let status = status_to_str(row.status);

        let conn = self.conn.lock();
        conn.execute(
            "INSERT INTO jobs (job_id, parent_job_id, tool, status, \
                progress_json, created_at, updated_at, error, result_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9) \
             ON CONFLICT(job_id) DO UPDATE SET \
                parent_job_id=excluded.parent_job_id, \
                tool=excluded.tool, \
                status=excluded.status, \
                progress_json=excluded.progress_json, \
                updated_at=excluded.updated_at, \
                error=excluded.error, \
                result_json=excluded.result_json",
            params![
                row.id,
                row.parent_job_id,
                row.tool_name,
                status,
                progress_json,
                row.created_at.to_rfc3339(),
                row.updated_at.to_rfc3339(),
                row.error,
                result_json,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Option<Job>, JobStorageError> {
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT job_id, parent_job_id, tool, status, progress_json, \
                        created_at, updated_at, error, result_json \
                 FROM jobs WHERE job_id = ?1",
                params![job_id],
                row_to_persisted,
            )
            .optional()
            .map_err(map_err)?;
        Ok(row.map(|r| r.to_job()))
    }

    fn list(&self, filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        let mut sql = String::from(
            "SELECT job_id, parent_job_id, tool, status, progress_json, \
                    created_at, updated_at, error, result_json \
             FROM jobs WHERE 1=1",
        );
        let mut bound: Vec<String> = Vec::new();
        if let Some(status) = filter.status {
            sql.push_str(" AND status = ?");
            bound.push(status_to_str(status).to_string());
        }
        if let Some(parent) = filter.parent_job_id.as_deref() {
            sql.push_str(" AND parent_job_id = ?");
            bound.push(parent.to_string());
        }
        sql.push_str(" ORDER BY created_at ASC");
        if let Some(limit) = filter.limit {
            sql.push_str(&format!(" LIMIT {limit}"));
        }

        let conn = self.conn.lock();
        let mut stmt = conn.prepare(&sql).map_err(map_err)?;
        let params_dyn: Vec<&dyn rusqlite::ToSql> =
            bound.iter().map(|s| s as &dyn rusqlite::ToSql).collect();
        let rows = stmt
            .query_map(params_dyn.as_slice(), row_to_persisted)
            .map_err(map_err)?;
        let mut out = Vec::new();
        for r in rows {
            out.push(r.map_err(map_err)?.to_job());
        }
        Ok(out)
    }

    fn update_status(
        &self,
        job_id: &str,
        status: JobStatus,
        at: DateTime<Utc>,
    ) -> Result<(), JobStorageError> {
        let conn = self.conn.lock();
        conn.execute(
            "UPDATE jobs SET status = ?1, updated_at = ?2 WHERE job_id = ?3",
            params![status_to_str(status), at.to_rfc3339(), job_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, JobStorageError> {
        let conn = self.conn.lock();
        // Only terminal jobs are eligible — mirror InMemoryStorage.
        let terminal = [
            status_to_str(JobStatus::Completed),
            status_to_str(JobStatus::Failed),
            status_to_str(JobStatus::Cancelled),
            status_to_str(JobStatus::Interrupted),
        ];
        let removed = conn
            .execute(
                "DELETE FROM jobs WHERE updated_at < ?1 AND status IN (?2, ?3, ?4, ?5)",
                params![
                    cutoff.to_rfc3339(),
                    terminal[0],
                    terminal[1],
                    terminal[2],
                    terminal[3],
                ],
            )
            .map_err(map_err)?;
        Ok(removed as u64)
    }
}

// ── Helpers ──────────────────────────────────────────────────────────────

fn map_err(e: rusqlite::Error) -> JobStorageError {
    JobStorageError::Backend(e.to_string())
}

fn status_to_str(s: JobStatus) -> &'static str {
    match s {
        JobStatus::Pending => "pending",
        JobStatus::Running => "running",
        JobStatus::Completed => "completed",
        JobStatus::Failed => "failed",
        JobStatus::Cancelled => "cancelled",
        JobStatus::Interrupted => "interrupted",
    }
}

fn str_to_status(s: &str) -> Result<JobStatus, JobStorageError> {
    Ok(match s {
        "pending" => JobStatus::Pending,
        "running" => JobStatus::Running,
        "completed" => JobStatus::Completed,
        "failed" => JobStatus::Failed,
        "cancelled" => JobStatus::Cancelled,
        "interrupted" => JobStatus::Interrupted,
        other => {
            return Err(JobStorageError::Decode(format!(
                "unknown job status in storage: {other}"
            )));
        }
    })
}

fn parse_rfc3339(s: &str) -> Result<DateTime<Utc>, JobStorageError> {
    DateTime::parse_from_rfc3339(s)
        .map(|d| d.with_timezone(&Utc))
        .map_err(|e| JobStorageError::Decode(format!("invalid rfc3339 timestamp `{s}`: {e}")))
}

fn row_to_persisted(row: &rusqlite::Row<'_>) -> rusqlite::Result<PersistedJob> {
    let id: String = row.get(0)?;
    let parent_job_id: Option<String> = row.get(1)?;
    let tool_name: String = row.get(2)?;
    let status_str: String = row.get(3)?;
    let progress_json: Option<String> = row.get(4)?;
    let created_at_str: String = row.get(5)?;
    let updated_at_str: String = row.get(6)?;
    let error: Option<String> = row.get(7)?;
    let result_json: Option<String> = row.get(8)?;

    // Map JobStorageError into rusqlite::Error so query_map's Result type
    // is satisfied; the wrapper at the top of each backend method
    // translates back to JobStorageError.
    let status = str_to_status(&status_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(3, rusqlite::types::Type::Text, e.into())
    })?;
    let progress: Option<JobProgress> = match progress_json {
        Some(s) => Some(serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(4, rusqlite::types::Type::Text, e.into())
        })?),
        None => None,
    };
    let result: Option<serde_json::Value> = match result_json {
        Some(s) => Some(serde_json::from_str(&s).map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
        })?),
        None => None,
    };
    let created_at = parse_rfc3339(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
    })?;
    let updated_at = parse_rfc3339(&updated_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, e.into())
    })?;

    Ok(PersistedJob {
        id,
        parent_job_id,
        tool_name,
        status,
        progress,
        result,
        error,
        created_at,
        updated_at,
    })
}

// ── Tests ────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobManager;
    use serde_json::json;

    #[test]
    fn sqlite_put_get_roundtrip() {
        let store = SqliteStorage::open_in_memory().unwrap();
        let mgr = JobManager::new();
        let handle = mgr.create("scene.get_info");
        let id = handle.read().id.clone();
        store.put(&handle.read()).unwrap();

        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.tool_name, "scene.get_info");
        assert_eq!(got.status, JobStatus::Pending);
    }

    #[test]
    fn sqlite_list_filter_by_status_and_parent() {
        let store = SqliteStorage::open_in_memory().unwrap();
        let mgr = JobManager::new();

        let parent = mgr.create("parent");
        let parent_id = parent.read().id.clone();
        store.put(&parent.read()).unwrap();

        let child = mgr.create_with_parent("child", Some(parent_id.clone()));
        let child_id = child.read().id.clone();
        store.put(&child.read()).unwrap();

        let standalone = mgr.create("lonely");
        store.put(&standalone.read()).unwrap();

        let children = store
            .list(JobFilter {
                parent_job_id: Some(parent_id.clone()),
                ..JobFilter::default()
            })
            .unwrap();
        assert_eq!(children.len(), 1);
        assert_eq!(children[0].id, child_id);

        let pending = store
            .list(JobFilter::by_status(JobStatus::Pending))
            .unwrap();
        assert_eq!(pending.len(), 3);
    }

    #[test]
    fn sqlite_update_status_and_crud() {
        let store = SqliteStorage::open_in_memory().unwrap();
        let mgr = JobManager::new();
        let handle = mgr.create("t");
        let id = handle.read().id.clone();
        store.put(&handle.read()).unwrap();

        store
            .update_status(&id, JobStatus::Running, Utc::now())
            .unwrap();
        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Running);

        // full put with a result field exercises the ON CONFLICT path
        {
            let mut job = handle.write();
            job.status = JobStatus::Completed;
            job.result = Some(json!({"ok": true}));
            job.updated_at = Utc::now();
        }
        store.put(&handle.read()).unwrap();
        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Completed);
        assert_eq!(got.result.as_ref().unwrap(), &json!({"ok": true}));
    }

    #[test]
    fn sqlite_delete_older_than_skips_non_terminal() {
        let store = SqliteStorage::open_in_memory().unwrap();
        let mgr = JobManager::new();

        let done = mgr.create("done");
        {
            let mut j = done.write();
            j.status = JobStatus::Completed;
            j.updated_at = Utc::now() - chrono::Duration::hours(24);
        }
        let done_id = done.read().id.clone();
        store.put(&done.read()).unwrap();

        let running = mgr.create("running");
        {
            let mut j = running.write();
            j.status = JobStatus::Running;
            j.updated_at = Utc::now() - chrono::Duration::hours(24);
        }
        let running_id = running.read().id.clone();
        store.put(&running.read()).unwrap();

        let removed = store
            .delete_older_than(Utc::now() - chrono::Duration::hours(1))
            .unwrap();
        assert_eq!(removed, 1);
        assert!(store.get(&done_id).unwrap().is_none());
        assert!(store.get(&running_id).unwrap().is_some());
    }
}
