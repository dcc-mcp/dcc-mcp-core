//! Pluggable persistence backend for [`JobManager`](crate::job::JobManager).
//!
//! Issue #328 — lets the server choose between the zero-dependency
//! [`InMemoryStorage`] (default) and the opt-in [`SqliteStorage`] (gated
//! behind the `job-persist-sqlite` Cargo feature).
//!
//! The storage layer is write-through: [`JobManager`] calls
//! [`JobStorage::put`] / [`JobStorage::update_status`] on every state
//! transition, so a crashed or restarted server can enumerate
//! pending/running rows and mark them as
//! [`JobStatus::Interrupted`](crate::job::JobStatus::Interrupted) — clients
//! never see a silently "lost" job.

use std::sync::Arc;

use chrono::{DateTime, Utc};
use parking_lot::Mutex;
use thiserror::Error;

use crate::job::{Job, JobProgress, JobStatus};

#[cfg(feature = "job-persist-sqlite")]
pub mod sqlite;

#[cfg(feature = "job-persist-sqlite")]
pub use sqlite::SqliteStorage;

/// Error returned by every [`JobStorage`] operation.
#[derive(Debug, Error)]
pub enum JobStorageError {
    /// Lower-level I/O or backend error (SQLite, filesystem, …).
    #[error("job storage backend error: {0}")]
    Backend(String),
    /// Row data failed to deserialize back into a [`Job`].
    #[error("failed to decode persisted job: {0}")]
    Decode(String),
    /// Config asked for a feature that is not compiled in.
    #[error(
        "job_storage_path is set but the `job-persist-sqlite` Cargo feature is \
         not enabled — rebuild dcc-mcp-core with `--features job-persist-sqlite` \
         or clear job_storage_path"
    )]
    FeatureDisabled,
}

/// Subset filter applied by [`JobStorage::list`].
///
/// All fields are additive — `None` means "no constraint on this axis".
#[derive(Debug, Clone, Default)]
pub struct JobFilter {
    /// Limit to a single status.
    pub status: Option<JobStatus>,
    /// Limit to children of a specific parent job.
    pub parent_job_id: Option<String>,
    /// Soft cap; `None` means unbounded.
    pub limit: Option<usize>,
}

impl JobFilter {
    /// Convenience: filter by status only.
    pub fn by_status(status: JobStatus) -> Self {
        Self {
            status: Some(status),
            ..Self::default()
        }
    }
}

/// Persistence interface for [`Job`] state.
///
/// Every implementation MUST be safe to share between Tokio worker
/// threads (`Send + Sync`) and MUST be write-through — readers of a
/// freshly restarted server depend on [`Self::list`] returning rows that
/// were `put` by the previous incarnation.
pub trait JobStorage: Send + Sync + std::fmt::Debug {
    /// Insert-or-update the full job row.
    fn put(&self, job: &Job) -> Result<(), JobStorageError>;

    /// Fetch a single job by id, if present.
    fn get(&self, job_id: &str) -> Result<Option<Job>, JobStorageError>;

    /// Enumerate jobs matching `filter`. Order is
    /// implementation-defined; callers must not rely on it.
    fn list(&self, filter: JobFilter) -> Result<Vec<Job>, JobStorageError>;

    /// Narrow write path for status transitions — more efficient than a
    /// full [`Self::put`] when the caller only touched `status` /
    /// `updated_at`. Implementations MAY fall back to a full put.
    fn update_status(
        &self,
        job_id: &str,
        status: JobStatus,
        at: DateTime<Utc>,
    ) -> Result<(), JobStorageError>;

    /// Delete terminal jobs whose `updated_at` is older than `cutoff`.
    /// Non-terminal jobs are never deleted. Returns the number of rows
    /// removed.
    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, JobStorageError>;
}

// ── In-memory implementation ─────────────────────────────────────────────

/// Zero-dependency default backend — keeps every row in a
/// `Mutex<HashMap>`. Used when `McpHttpConfig::job_storage_path` is
/// `None`.
///
/// The in-memory store has no restart-recovery story (the process dies
/// and the map is gone) — that is deliberate: the whole point of the
/// SQLite feature is to provide persistence only for deployments that
/// actually need it.
#[derive(Debug, Default)]
pub struct InMemoryStorage {
    // Store serialisable snapshot so `list` round-trips identically to
    // the SQLite backend (no sharing of `CancellationToken` here; the
    // consumer is always `JobManager::recover_from_storage` which
    // re-wraps rows into fresh `Interrupted` terminal-state snapshots).
    rows: Mutex<std::collections::HashMap<String, PersistedJob>>,
}

impl InMemoryStorage {
    /// Create an empty in-memory store.
    pub fn new() -> Self {
        Self::default()
    }

    /// Convenience wrapper used by [`JobManager::with_default_storage`](crate::job::JobManager::with_default_storage).
    pub fn shared() -> Arc<Self> {
        Arc::new(Self::default())
    }
}

impl JobStorage for InMemoryStorage {
    fn put(&self, job: &Job) -> Result<(), JobStorageError> {
        let row = PersistedJob::from_job(job);
        self.rows.lock().insert(job.id.clone(), row);
        Ok(())
    }

    fn get(&self, job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(self.rows.lock().get(job_id).map(PersistedJob::to_job))
    }

    fn list(&self, filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        let rows = self.rows.lock();
        let mut out: Vec<Job> = rows
            .values()
            .filter(|r| {
                filter.status.map(|s| r.status == s).unwrap_or(true)
                    && filter
                        .parent_job_id
                        .as_deref()
                        .map(|p| r.parent_job_id.as_deref() == Some(p))
                        .unwrap_or(true)
            })
            .map(PersistedJob::to_job)
            .collect();
        if let Some(limit) = filter.limit {
            out.truncate(limit);
        }
        Ok(out)
    }

    fn update_status(
        &self,
        job_id: &str,
        status: JobStatus,
        at: DateTime<Utc>,
    ) -> Result<(), JobStorageError> {
        let mut rows = self.rows.lock();
        if let Some(row) = rows.get_mut(job_id) {
            row.status = status;
            row.updated_at = at;
        }
        Ok(())
    }

    fn delete_older_than(&self, cutoff: DateTime<Utc>) -> Result<u64, JobStorageError> {
        let mut rows = self.rows.lock();
        let mut removed = 0u64;
        rows.retain(|_, r| {
            let should_delete = r.status.is_terminal() && r.updated_at < cutoff;
            if should_delete {
                removed += 1;
            }
            !should_delete
        });
        Ok(removed)
    }
}

// ── Wire-format helper ──────────────────────────────────────────────────

/// Serialisable snapshot of a [`Job`] used by every backend.
///
/// Deliberately flat + `Clone` so both [`InMemoryStorage`] and the
/// SQLite backend produce the same JSON for introspection.
#[derive(Debug, Clone)]
pub struct PersistedJob {
    pub id: String,
    pub parent_job_id: Option<String>,
    pub tool_name: String,
    pub status: JobStatus,
    pub progress: Option<JobProgress>,
    pub result: Option<serde_json::Value>,
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
}

impl PersistedJob {
    pub fn from_job(job: &Job) -> Self {
        Self {
            id: job.id.clone(),
            parent_job_id: job.parent_job_id.clone(),
            tool_name: job.tool_name.clone(),
            status: job.status,
            progress: job.progress.clone(),
            result: job.result.clone(),
            error: job.error.clone(),
            created_at: job.created_at,
            updated_at: job.updated_at,
        }
    }

    /// Rehydrate a [`Job`] from persisted state. Since
    /// `CancellationToken` is not serialisable we attach a fresh
    /// (already-cancelled on terminal rows) token — consumers only use
    /// recovered rows for read-only reporting.
    pub fn to_job(&self) -> Job {
        let cancel_token = tokio_util::sync::CancellationToken::new();
        if self.status.is_terminal() {
            cancel_token.cancel();
        }
        Job {
            id: self.id.clone(),
            tool_name: self.tool_name.clone(),
            status: self.status,
            parent_job_id: self.parent_job_id.clone(),
            progress: self.progress.clone(),
            result: self.result.clone(),
            error: self.error.clone(),
            created_at: self.created_at,
            updated_at: self.updated_at,
            cancel_token,
        }
    }
}

// ── Tests ───────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use crate::job::JobManager;
    use serde_json::json;

    fn sample_job(mgr: &JobManager, tool: &str) -> String {
        mgr.create(tool).read().id.clone()
    }

    #[test]
    fn inmemory_put_get_roundtrip() {
        let store = InMemoryStorage::new();
        let mgr = JobManager::new();
        let id = sample_job(&mgr, "scene.get_info");
        let job = mgr.get(&id).unwrap();
        let snap = job.read().clone();
        store.put(&snap).unwrap();

        let got = store.get(&id).unwrap().unwrap();
        assert_eq!(got.id, id);
        assert_eq!(got.tool_name, "scene.get_info");
        assert_eq!(got.status, JobStatus::Pending);
    }

    #[test]
    fn inmemory_update_status_and_list_filter() {
        let store = InMemoryStorage::new();
        let mgr = JobManager::new();
        let a = sample_job(&mgr, "a");
        let b = sample_job(&mgr, "b");
        for id in [&a, &b] {
            store.put(&mgr.get(id).unwrap().read()).unwrap();
        }
        store
            .update_status(&a, JobStatus::Running, Utc::now())
            .unwrap();
        let running = store
            .list(JobFilter::by_status(JobStatus::Running))
            .unwrap();
        assert_eq!(running.len(), 1);
        assert_eq!(running[0].id, a);

        let pending = store
            .list(JobFilter::by_status(JobStatus::Pending))
            .unwrap();
        assert_eq!(pending.len(), 1);
        assert_eq!(pending[0].id, b);
    }

    #[test]
    fn inmemory_delete_older_than_skips_non_terminal() {
        let store = InMemoryStorage::new();
        let mgr = JobManager::new();
        let done_id = sample_job(&mgr, "done");
        let running_id = sample_job(&mgr, "running");

        {
            let handle = mgr.get(&done_id).unwrap();
            let mut job = handle.write();
            job.status = JobStatus::Completed;
            job.result = Some(json!({"ok": true}));
            job.updated_at = Utc::now() - chrono::Duration::hours(24);
            store.put(&job).unwrap();
        }
        {
            let handle = mgr.get(&running_id).unwrap();
            let mut job = handle.write();
            job.status = JobStatus::Running;
            job.updated_at = Utc::now() - chrono::Duration::hours(24);
            store.put(&job).unwrap();
        }

        let cutoff = Utc::now() - chrono::Duration::hours(1);
        let removed = store.delete_older_than(cutoff).unwrap();
        assert_eq!(removed, 1);
        assert!(store.get(&done_id).unwrap().is_none());
        assert!(store.get(&running_id).unwrap().is_some());
    }
}
