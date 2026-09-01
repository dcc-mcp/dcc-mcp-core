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

use std::fs::{File, OpenOptions};
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::sync::atomic::{AtomicBool, Ordering};
use std::time::Duration;

use chrono::{DateTime, Utc};
use fs4::{FileExt, TryLockError};
use parking_lot::{Mutex, RwLock};
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
    started_at TEXT,
    completed_at TEXT,
    updated_at TEXT NOT NULL,
    error TEXT,
    result_json TEXT
);
CREATE INDEX IF NOT EXISTS jobs_status_idx ON jobs(status);
CREATE INDEX IF NOT EXISTS jobs_parent_idx ON jobs(parent_job_id);
CREATE INDEX IF NOT EXISTS jobs_updated_idx ON jobs(updated_at);
"#;

/// SQLite-backed [`JobStorage`].
pub struct SqliteStorage {
    conn: Mutex<Connection>,
    interrupt: rusqlite::InterruptHandle,
    lock_file: Arc<Mutex<Option<File>>>,
    lifecycle: Arc<RwLock<()>>,
    closed: AtomicBool,
    handoff_fence_started: AtomicBool,
}

impl std::fmt::Debug for SqliteStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("SqliteStorage")
            .finish_non_exhaustive()
    }
}

impl SqliteStorage {
    /// Open (or create) a SQLite database at `path` and run schema
    /// migrations. Parent directories are created as needed.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, JobStorageError> {
        let input_path = path.as_ref();
        if let Ok(meta) = std::fs::symlink_metadata(input_path)
            && meta.file_type().is_symlink()
        {
            return Err(JobStorageError::Backend(
                "job storage path must not be a symlink or reparse point".to_string(),
            ));
        }
        if let Some(parent) = input_path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| {
                JobStorageError::Backend(format!(
                    "failed to create parent directory for job storage: {e}"
                ))
            })?;
        }
        // Materialize the database before deriving its ownership key so
        // aliases (including hardlinks) resolve to one physical identity.
        if !input_path.exists() {
            OpenOptions::new()
                .create(true)
                .truncate(false)
                .write(true)
                .open(input_path)
                .map_err(|error| {
                    JobStorageError::Backend(format!(
                        "failed to create job storage database: {error}"
                    ))
                })?;
        }
        // Resolve the physical path once and use it for both ownership and
        // SQLite opening. This closes the path-alias TOCTOU window between
        // deriving the sidecar key and opening the database.
        let path = std::fs::canonicalize(input_path).map_err(|error| {
            JobStorageError::Backend(format!("failed to resolve job storage path: {error}"))
        })?;
        let lock_path = ownership_lock_path(&path);
        let lock_file = OpenOptions::new()
            .create(true)
            .truncate(false)
            .read(true)
            .write(true)
            .open(&lock_path)
            .map_err(|error| {
                JobStorageError::Backend(format!(
                    "failed to open job storage ownership lock: {error}"
                ))
            })?;
        match FileExt::try_lock(&lock_file) {
            Ok(()) => {}
            Err(TryLockError::WouldBlock) => {
                return Err(JobStorageError::Backend(
                    "job storage ownership unavailable: another process holds the database lock"
                        .to_string(),
                ));
            }
            Err(TryLockError::Error(error)) => {
                return Err(JobStorageError::Backend(format!(
                    "failed to acquire job storage ownership lock: {error}"
                )));
            }
        }
        let conn = Connection::open(&path).map_err(map_err)?;
        let interrupt = conn.get_interrupt_handle();
        // Keep cross-process writers from failing immediately on a short
        // SQLite lock window. The persistence circuit still latches a
        // sustained failure; this timeout only gives another process time to
        // finish its transaction.
        conn.busy_timeout(Duration::from_secs(5)).map_err(map_err)?;

        // WAL is preferred for concurrent readers, but changing journal mode
        // needs a writable database and sidecar files. A database can remain
        // readable after its permissions change or after stale -wal/-shm
        // files are left by another process. Treat journal-mode setup as a
        // best-effort optimization: preserve the connection and let the
        // write circuit report/disable persistence if writes are unavailable.
        if let Err(error) = conn.execute_batch("PRAGMA journal_mode=WAL;") {
            tracing::warn!(error = %error, "SQLite WAL setup unavailable; continuing with the existing journal mode");
        }
        conn.execute_batch("PRAGMA synchronous=NORMAL; PRAGMA foreign_keys=ON;")
            .map_err(map_err)?;
        conn.execute_batch(SCHEMA).map_err(map_err)?;
        ensure_lifecycle_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            interrupt,
            lock_file: Arc::new(Mutex::new(Some(lock_file))),
            lifecycle: Arc::new(RwLock::new(())),
            closed: AtomicBool::new(false),
            handoff_fence_started: AtomicBool::new(false),
        })
    }

    /// Open an in-memory database — used by tests that want the real
    /// SQLite code path without touching the filesystem.
    pub fn open_in_memory() -> Result<Self, JobStorageError> {
        let conn = Connection::open_in_memory().map_err(map_err)?;
        let interrupt = conn.get_interrupt_handle();
        conn.execute_batch(SCHEMA).map_err(map_err)?;
        ensure_lifecycle_columns(&conn)?;
        Ok(Self {
            conn: Mutex::new(conn),
            interrupt,
            lock_file: Arc::new(Mutex::new(None)),
            lifecycle: Arc::new(RwLock::new(())),
            closed: AtomicBool::new(false),
            handoff_fence_started: AtomicBool::new(false),
        })
    }

    fn ensure_open(&self) -> Result<parking_lot::RwLockReadGuard<'_, ()>, JobStorageError> {
        let guard = self.lifecycle.read();
        if self.closed.load(Ordering::Acquire) {
            return Err(JobStorageError::Backend(
                "job storage is closed after server shutdown".to_string(),
            ));
        }
        Ok(guard)
    }
}

impl Drop for SqliteStorage {
    fn drop(&mut self) {
        if let Some(lock_file) = self.lock_file.lock().take() {
            let _ = FileExt::unlock(&lock_file);
        }
    }
}

impl JobStorage for SqliteStorage {
    fn shutdown(&self) {
        let _ = self.shutdown_with_timeout(Duration::from_secs(5));
    }

    fn shutdown_with_timeout(&self, timeout: Duration) -> bool {
        let Some(_lifecycle) = self.lifecycle.try_write_for(timeout) else {
            return self.force_close();
        };
        self.closed.store(true, Ordering::Release);
        if let Some(lock_file) = self.lock_file.lock().take() {
            let _ = FileExt::unlock(&lock_file);
        }
        true
    }

    fn force_close(&self) -> bool {
        self.closed.store(true, Ordering::Release);
        // Abort a statement that is blocked in SQLite's busy handler. The
        // ownership lease remains held until every operation that passed the
        // closed check has dropped its lifecycle guard; otherwise a
        // replacement could acquire the lease before a pre-statement writer
        // is fenced.
        self.interrupt.interrupt();
        if let Some(_lifecycle) = self.lifecycle.try_write() {
            if let Some(lock_file) = self.lock_file.lock().take() {
                let _ = FileExt::unlock(&lock_file);
            }
            return true;
        }

        if self
            .handoff_fence_started
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            let lifecycle = Arc::clone(&self.lifecycle);
            let lock_file = Arc::clone(&self.lock_file);
            if std::thread::Builder::new()
                .name("dcc-mcp-job-sqlite-close".to_string())
                .spawn(move || {
                    let _lifecycle = lifecycle.write();
                    if let Some(lock_file) = lock_file.lock().take() {
                        let _ = FileExt::unlock(&lock_file);
                    }
                })
                .is_err()
            {
                self.handoff_fence_started.store(false, Ordering::Release);
            }
        }
        false
    }

    fn put(&self, job: &Job) -> Result<(), JobStorageError> {
        let _lifecycle = self.ensure_open()?;
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
        if self.closed.load(Ordering::Acquire) {
            return Err(JobStorageError::Backend(
                "job storage is closed after server shutdown".to_string(),
            ));
        }
        conn.execute(
            "INSERT INTO jobs (job_id, parent_job_id, tool, status, \
                progress_json, created_at, started_at, completed_at, updated_at, error, result_json) \
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, ?11) \
             ON CONFLICT(job_id) DO UPDATE SET \
                parent_job_id=excluded.parent_job_id, \
                tool=excluded.tool, \
                status=excluded.status, \
                progress_json=excluded.progress_json, \
                started_at=excluded.started_at, \
                completed_at=excluded.completed_at, \
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
                row.started_at.map(|at| at.to_rfc3339()),
                row.completed_at.map(|at| at.to_rfc3339()),
                row.updated_at.to_rfc3339(),
                row.error,
                result_json,
            ],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Option<Job>, JobStorageError> {
        let _lifecycle = self.ensure_open()?;
        let conn = self.conn.lock();
        let row = conn
            .query_row(
                "SELECT job_id, parent_job_id, tool, status, progress_json, \
                        created_at, started_at, completed_at, updated_at, error, result_json \
                 FROM jobs WHERE job_id = ?1",
                params![job_id],
                row_to_persisted,
            )
            .optional()
            .map_err(map_err)?;
        Ok(row.map(|r| r.to_job()))
    }

    fn list(&self, filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        let _lifecycle = self.ensure_open()?;
        let mut sql = String::from(
            "SELECT job_id, parent_job_id, tool, status, progress_json, \
                    created_at, started_at, completed_at, updated_at, error, result_json \
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
        let _lifecycle = self.ensure_open()?;
        let conn = self.conn.lock();
        if self.closed.load(Ordering::Acquire) {
            return Err(JobStorageError::Backend(
                "job storage is closed after server shutdown".to_string(),
            ));
        }
        conn.execute(
            "UPDATE jobs SET \
                status = ?1, \
                updated_at = ?2, \
                started_at = CASE \
                    WHEN ?1 = 'running' THEN COALESCE(started_at, ?2) \
                    ELSE started_at \
                END, \
                completed_at = CASE \
                    WHEN ?1 IN ('completed', 'failed', 'cancelled', 'interrupted') THEN ?2 \
                    ELSE completed_at \
                END \
             WHERE job_id = ?3",
            params![status_to_str(status), at.to_rfc3339(), job_id],
        )
        .map_err(map_err)?;
        Ok(())
    }

    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, JobStorageError> {
        let _lifecycle = self.ensure_open()?;
        let conn = self.conn.lock();
        if self.closed.load(Ordering::Acquire) {
            return Err(JobStorageError::Backend(
                "job storage is closed after server shutdown".to_string(),
            ));
        }
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

pub fn ownership_lock_path_for(path: impl AsRef<Path>) -> PathBuf {
    ownership_lock_path(path.as_ref())
}

fn ownership_lock_path(path: &Path) -> PathBuf {
    if let Some(identity) = physical_identity(path) {
        // Keep the lease outside the database directory so symlink and
        // hardlink aliases converge on one OS lock even when parent paths
        // differ.
        return std::env::temp_dir().join(format!("dcc-mcp-job-{identity}.lock"));
    }
    let mut lock_name = path
        .file_name()
        .map(|name| name.to_os_string())
        .unwrap_or_else(|| "jobs.sqlite3".into());
    lock_name.push(".lock");
    path.with_file_name(lock_name)
}

fn windows_physical_identity(volume: u32, file_index_high: u32, file_index_low: u32) -> String {
    format!("w{volume:08x}-{file_index_high:08x}-{file_index_low:08x}")
}

fn physical_identity(path: &Path) -> Option<String> {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        let metadata = std::fs::metadata(path).ok()?;
        return Some(format!("u{:x}-{:x}", metadata.dev(), metadata.ino()));
    }
    #[cfg(windows)]
    {
        use std::os::windows::io::AsRawHandle;
        let file = OpenOptions::new().read(true).open(path).ok()?;
        let mut info = std::mem::MaybeUninit::<
            windows_sys::Win32::Storage::FileSystem::BY_HANDLE_FILE_INFORMATION,
        >::zeroed();
        let ok = unsafe {
            windows_sys::Win32::Storage::FileSystem::GetFileInformationByHandle(
                file.as_raw_handle() as _,
                info.as_mut_ptr(),
            )
        };
        if ok == 0 {
            return None;
        }
        let info = unsafe { info.assume_init() };
        return Some(windows_physical_identity(
            info.dwVolumeSerialNumber,
            info.nFileIndexHigh,
            info.nFileIndexLow,
        ));
    }
    #[allow(unreachable_code)]
    {
        std::fs::canonicalize(path).ok().map(|resolved| {
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            resolved.to_string_lossy().hash(&mut hasher);
            format!("p{:x}", hasher.finish())
        })
    }
}

fn ensure_lifecycle_columns(conn: &Connection) -> Result<(), JobStorageError> {
    for column in ["started_at", "completed_at"] {
        let exists = conn
            .query_row(
                "SELECT 1 FROM pragma_table_info('jobs') WHERE name = ?1",
                params![column],
                |_| Ok(()),
            )
            .optional()
            .map_err(map_err)?
            .is_some();
        if !exists {
            conn.execute(&format!("ALTER TABLE jobs ADD COLUMN {column} TEXT"), [])
                .map_err(map_err)?;
        }
    }
    Ok(())
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
    let started_at_str: Option<String> = row.get(6)?;
    let completed_at_str: Option<String> = row.get(7)?;
    let updated_at_str: String = row.get(8)?;
    let error: Option<String> = row.get(9)?;
    let result_json: Option<String> = row.get(10)?;

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
            rusqlite::Error::FromSqlConversionFailure(10, rusqlite::types::Type::Text, e.into())
        })?),
        None => None,
    };
    let created_at = parse_rfc3339(&created_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(5, rusqlite::types::Type::Text, e.into())
    })?;
    let started_at = started_at_str
        .as_deref()
        .map(parse_rfc3339)
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(6, rusqlite::types::Type::Text, e.into())
        })?;
    let completed_at = completed_at_str
        .as_deref()
        .map(parse_rfc3339)
        .transpose()
        .map_err(|e| {
            rusqlite::Error::FromSqlConversionFailure(7, rusqlite::types::Type::Text, e.into())
        })?;
    let updated_at = parse_rfc3339(&updated_at_str).map_err(|e| {
        rusqlite::Error::FromSqlConversionFailure(8, rusqlite::types::Type::Text, e.into())
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
        started_at,
        completed_at,
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

        let started_at = Utc::now();
        store
            .update_status(&id, JobStatus::Running, started_at)
            .unwrap();
        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Running);
        assert_eq!(got.started_at, Some(started_at));
        assert!(got.completed_at.is_none());

        // full put with a result field exercises the ON CONFLICT path
        {
            let mut job = handle.write();
            job.status = JobStatus::Completed;
            job.result = Some(json!({"ok": true}));
            job.started_at = Some(started_at);
            job.completed_at = Some(Utc::now());
            job.updated_at = job.completed_at.unwrap();
        }
        store.put(&handle.read()).unwrap();
        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.status, JobStatus::Completed);
        assert_eq!(got.result.as_ref().unwrap(), &json!({"ok": true}));
        assert_eq!(got.started_at, Some(started_at));
        assert_eq!(got.completed_at, Some(got.updated_at));
    }

    #[test]
    fn sqlite_open_migrates_existing_job_table() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-lifecycle-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        {
            let conn = Connection::open(&path).unwrap();
            conn.execute_batch(
                "CREATE TABLE jobs (\
                    job_id TEXT PRIMARY KEY, parent_job_id TEXT, tool TEXT NOT NULL, \
                    status TEXT NOT NULL, progress_json TEXT, created_at TEXT NOT NULL, \
                    updated_at TEXT NOT NULL, error TEXT, result_json TEXT\
                );",
            )
            .unwrap();
        }

        let store = SqliteStorage::open(&path).unwrap();
        let mgr = JobManager::new();
        let handle = mgr.create("render.sequence");
        let id = handle.read().id.clone();
        mgr.start(&id).unwrap();
        mgr.complete(&id, json!({"ok": true})).unwrap();
        store.put(&handle.read()).unwrap();

        let got = store.get(&id).unwrap().unwrap();
        assert!(got.started_at.is_some());
        assert_eq!(got.completed_at, Some(got.updated_at));
        drop(store);
        let _ = std::fs::remove_file(path);
    }

    #[test]
    fn sqlite_open_configures_bounded_busy_timeout() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-busy-timeout-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let store = SqliteStorage::open(&path).unwrap();
        let timeout_ms: i64 = store
            .conn
            .lock()
            .query_row("PRAGMA busy_timeout", [], |row| row.get(0))
            .unwrap();
        assert_eq!(timeout_ms, 5_000);
        drop(store);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
    }

    #[test]
    fn sqlite_open_rejects_a_second_process_owner() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let first = SqliteStorage::open(&path).unwrap();
        let second = SqliteStorage::open(&path);
        assert!(
            second
                .expect_err("a second process must not share the writable job database")
                .to_string()
                .contains("ownership")
        );
        drop(first);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
    }

    #[test]
    fn sqlite_open_rejects_a_hardlink_alias_of_the_same_database() {
        let root = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-hardlink-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.with_extension("sqlite3");
        let alias = root.with_extension("alias.sqlite3");
        let first = SqliteStorage::open(&path).unwrap();
        std::fs::hard_link(&path, &alias).unwrap();
        let second = SqliteStorage::open(&alias);
        assert!(
            second
                .expect_err("a hardlink alias must share database ownership")
                .to_string()
                .contains("ownership")
        );
        drop(first);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&alias);
    }

    #[cfg(unix)]
    #[test]
    fn sqlite_open_rejects_a_symlink_alias_before_locking() {
        let root = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-symlink-{}",
            uuid::Uuid::new_v4()
        ));
        let path = root.with_extension("sqlite3");
        let alias = root.with_extension("alias.sqlite3");
        let first = SqliteStorage::open(&path).unwrap();
        std::os::unix::fs::symlink(&path, &alias).unwrap();
        let second = SqliteStorage::open(&alias);
        assert!(second.is_err(), "symlink aliases must fail closed");
        drop(first);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(&alias);
    }

    #[test]
    fn sqlite_shutdown_releases_owner_and_fails_closed() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-shutdown-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let first = SqliteStorage::open(&path).unwrap();
        first.shutdown();

        let second = SqliteStorage::open(&path)
            .expect("a stopped server must release its database ownership lease");
        let mgr = JobManager::new();
        let job = mgr.create("scene.inspect");
        let error = first
            .put(&job.read())
            .expect_err("closed storage must reject detached background writes");
        assert!(error.to_string().contains("closed after server shutdown"));

        drop(second);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
    }

    #[test]
    fn sqlite_bounded_shutdown_retries_after_lifecycle_reader_releases() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-shutdown-timeout-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let storage = std::sync::Arc::new(SqliteStorage::open(&path).unwrap());
        let reader = storage.lifecycle.read();
        let blocked = std::sync::Arc::clone(&storage);
        let result =
            std::thread::spawn(move || blocked.shutdown_with_timeout(Duration::from_millis(10)))
                .join()
                .unwrap();
        assert!(
            !result,
            "shutdown must retain ownership while a lifecycle reader is active"
        );
        assert!(
            SqliteStorage::open(&path).is_err(),
            "a replacement must not acquire ownership before the old reader quiesces"
        );
        drop(reader);
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let second = loop {
            match SqliteStorage::open(&path) {
                Ok(storage) => break storage,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Err(error) => panic!("ownership must be reacquirable after quiescence: {error}"),
            }
        };
        drop(second);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
    }

    #[test]
    fn windows_physical_identity_encoding_is_injective() {
        let first = windows_physical_identity(0x77, 0x1, 0x23);
        let second = windows_physical_identity(0x77, 0x12, 0x3);
        assert_ne!(first, second);
        assert_eq!(first, "w00000077-00000001-00000023");
        assert_eq!(second, "w00000077-00000012-00000003");
    }

    #[test]
    fn force_close_does_not_handoff_before_prechecked_writer_quiesces() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-prechecked-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let storage = std::sync::Arc::new(SqliteStorage::open(&path).unwrap());

        // Model a mutator after its final closed check but before conn.execute.
        let lifecycle = storage.lifecycle.read();
        let conn = storage.conn.lock();
        assert!(!storage.closed.load(Ordering::Acquire));

        let closing = std::sync::Arc::clone(&storage);
        assert!(
            !std::thread::spawn(move || {
                closing.shutdown_with_timeout(Duration::from_millis(10))
            })
            .join()
            .unwrap(),
            "bounded shutdown cannot release ownership before the writer quiesces"
        );
        assert!(
            SqliteStorage::open(&path).is_err(),
            "replacement ownership must remain fenced"
        );

        assert!(storage.closed.load(Ordering::Acquire));
        drop(conn);
        drop(lifecycle);
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let replacement = loop {
            match SqliteStorage::open(&path) {
                Ok(storage) => break storage,
                Err(_) if std::time::Instant::now() < deadline => {
                    std::thread::yield_now();
                }
                Err(error) => panic!("replacement must acquire after quiescence: {error}"),
            }
        };
        let replacement_job = JobManager::new().create("replacement.write");
        replacement.put(&replacement_job.read()).unwrap();

        drop(replacement);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
    }

    #[test]
    fn sqlite_shutdown_forces_sidecar_release_after_blocked_write() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-busy-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let storage = std::sync::Arc::new(SqliteStorage::open(&path).unwrap());
        let blocker = Connection::open(&path).unwrap();
        blocker.busy_timeout(Duration::from_secs(30)).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let job = JobManager::new().create("blocked.write");
        let writer_storage = std::sync::Arc::clone(&storage);
        let writer_job = job.read().clone();
        let writer = std::thread::spawn(move || writer_storage.put(&writer_job));
        std::thread::sleep(Duration::from_millis(50));

        // The lifecycle guard is held by the blocked writer. Shutdown marks
        // the handle closed but must retain ownership until that writer
        // quiesces.
        assert!(!storage.shutdown_with_timeout(Duration::from_millis(10)));
        assert!(SqliteStorage::open(&path).is_err());
        blocker.execute_batch("ROLLBACK").unwrap();

        let writer_error = writer
            .join()
            .expect("blocked writer thread must terminate")
            .expect_err("force-close must fence the blocked write");
        assert!(
            writer_error
                .to_string()
                .contains("closed after server shutdown")
                || writer_error.to_string().contains("interrupted")
                || writer_error.to_string().contains("database is locked")
        );

        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let second = loop {
            match SqliteStorage::open(&path) {
                Ok(storage) => break storage,
                Err(_) if std::time::Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => panic!("ownership must release after the writer quiesces: {error}"),
            }
        };
        let replacement = JobManager::new().create("replacement.write");
        second.put(&replacement.read()).unwrap();

        assert!(second.get(&job.read().id).unwrap().is_none());
        drop(second);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
    }

    #[test]
    fn dropping_offloaded_manager_releases_sqlite_lease_after_blocked_write() {
        let path = std::env::temp_dir().join(format!(
            "dcc-mcp-job-owner-drop-{}.sqlite3",
            uuid::Uuid::new_v4()
        ));
        let storage = std::sync::Arc::new(SqliteStorage::open(&path).unwrap());
        let blocker = Connection::open(&path).unwrap();
        blocker.busy_timeout(Duration::from_secs(30)).unwrap();
        blocker.execute_batch("BEGIN EXCLUSIVE").unwrap();

        let jobs = JobManager::with_offloaded_storage(storage.clone());
        let job = jobs.create("blocked.drop");
        std::thread::sleep(Duration::from_millis(50));
        drop(jobs);

        assert!(
            SqliteStorage::open(&path).is_err(),
            "the lease must remain held while the old write is active"
        );
        blocker.execute_batch("ROLLBACK").unwrap();
        let deadline = std::time::Instant::now() + Duration::from_secs(1);
        let replacement = loop {
            match SqliteStorage::open(&path) {
                Ok(storage) => break storage,
                Err(_) if std::time::Instant::now() < deadline => std::thread::yield_now(),
                Err(error) => {
                    panic!("dropping a blocked manager must release after drain: {error}")
                }
            }
        };
        let replacement_job = JobManager::new().create("replacement.drop");
        replacement.put(&replacement_job.read()).unwrap();
        assert!(replacement.get(&job.read().id).unwrap().is_none());

        drop(replacement);
        drop(storage);
        let _ = std::fs::remove_file(&path);
        let _ = std::fs::remove_file(path.with_extension("sqlite3-wal"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3-shm"));
        let _ = std::fs::remove_file(path.with_extension("sqlite3.lock"));
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
