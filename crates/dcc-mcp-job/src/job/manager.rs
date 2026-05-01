//! The `JobManager` registry.
//!
//! See [`crate::job`] for the module-level overview and state diagram.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use tokio_util::sync::CancellationToken;
use tracing::debug;

use super::types::{Job, JobEvent, JobProgress, JobStatus, JobSubscriber};

/// Thread-safe registry of [`Job`]s.
#[derive(Default)]
pub struct JobManager {
    jobs: DashMap<String, Arc<RwLock<Job>>>,
    /// Subscribers invoked on every status transition. See [`Self::subscribe`].
    subscribers: RwLock<Vec<JobSubscriber>>,
    /// Optional persistence backend (issue #328). When `Some`, every
    /// mutation is written through to storage so the next process
    /// incarnation can see and mark-interrupted any in-flight jobs.
    storage: Option<Arc<dyn crate::job_storage::JobStorage>>,
}

impl std::fmt::Debug for JobManager {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JobManager")
            .field("jobs", &self.jobs.len())
            .field("subscribers", &self.subscribers.read().len())
            .field("has_storage", &self.storage.is_some())
            .finish()
    }
}

impl JobManager {
    /// Create an empty manager with no persistence backend.
    pub fn new() -> Self {
        Self {
            jobs: DashMap::new(),
            subscribers: RwLock::new(Vec::new()),
            storage: None,
        }
    }

    /// Create a manager that writes every mutation through to `storage`
    /// (issue #328).
    ///
    /// Does NOT perform recovery automatically — call
    /// [`Self::recover_from_storage`] once after construction if the
    /// backend may already contain rows from a previous process.
    pub fn with_storage(storage: Arc<dyn crate::job_storage::JobStorage>) -> Self {
        Self {
            jobs: DashMap::new(),
            subscribers: RwLock::new(Vec::new()),
            storage: Some(storage),
        }
    }

    /// Attach a storage backend to an existing manager. Intended for
    /// uncommon build-up paths; the primary entry point is
    /// [`Self::with_storage`].
    pub fn set_storage(&mut self, storage: Arc<dyn crate::job_storage::JobStorage>) {
        self.storage = Some(storage);
    }

    /// Borrow the underlying storage, if any.
    pub fn storage(&self) -> Option<Arc<dyn crate::job_storage::JobStorage>> {
        self.storage.clone()
    }

    /// Recover any in-flight rows left over by a previous process
    /// (issue #328). Every row whose status is `Pending` or `Running`
    /// is rewritten to [`JobStatus::Interrupted`] with
    /// `error = "server restart"` and made visible to subscribers so
    /// the `$/dcc.jobUpdated` SSE channel surfaces the transition.
    ///
    /// Terminal rows are left untouched — they remain queryable via
    /// `jobs.get_status` / `jobs.cleanup`.
    ///
    /// Returns the number of rows that were flipped to `Interrupted`.
    pub fn recover_from_storage(&self) -> Result<usize, crate::job_storage::JobStorageError> {
        let storage = match &self.storage {
            Some(s) => Arc::clone(s),
            None => return Ok(0),
        };
        let all = storage.list(crate::job_storage::JobFilter::default())?;
        let mut interrupted = 0usize;
        let now = Utc::now();
        for mut job in all {
            let was_inflight = matches!(job.status, JobStatus::Pending | JobStatus::Running);
            if was_inflight {
                job.status = JobStatus::Interrupted;
                job.error = Some("server restart".to_string());
                job.updated_at = now;
                // Persist the new terminal state before we hand the row
                // back out so a second crash does not re-flip it.
                storage.put(&job)?;
                interrupted += 1;
            }
            // Rehydrate the in-process map so reads and the next
            // `gc_stale` pass see recovered rows.
            let id = job.id.clone();
            let should_emit = was_inflight;
            let snapshot = job.clone();
            self.jobs.insert(id, Arc::new(RwLock::new(job)));
            if should_emit {
                self.emit(&snapshot);
            }
        }
        Ok(interrupted)
    }

    fn persist_put(&self, job: &Job) {
        if let Some(storage) = &self.storage
            && let Err(e) = storage.put(job)
        {
            tracing::warn!(job_id = %job.id, error = %e, "JobStorage.put failed");
        }
    }

    fn persist_status(&self, job_id: &str, status: JobStatus, at: DateTime<Utc>) {
        if let Some(storage) = &self.storage
            && let Err(e) = storage.update_status(job_id, status, at)
        {
            tracing::warn!(job_id = %job_id, error = %e, "JobStorage.update_status failed");
        }
    }

    /// Register a subscriber invoked on every status transition.
    ///
    /// Subscribers are called synchronously while the internal write lock is
    /// held, so they MUST be cheap and non-blocking. The notification
    /// layer (#326) queues events onto a `broadcast::Sender` inside the
    /// callback — it never performs I/O itself.
    pub fn subscribe<F>(&self, f: F)
    where
        F: Fn(JobEvent) + Send + Sync + 'static,
    {
        self.subscribers.write().push(Arc::new(f));
    }

    fn emit(&self, job: &Job) {
        let event = JobEvent {
            id: job.id.clone(),
            tool_name: job.tool_name.clone(),
            status: job.status,
            progress: job.progress.clone(),
            error: job.error.clone(),
            updated_at: job.updated_at,
            created_at: job.created_at,
        };
        let subs = self.subscribers.read().clone();
        for sub in subs {
            sub(event.clone());
        }
    }

    /// Create a new job in the `Pending` state and return a handle to it.
    ///
    /// Convenience wrapper for the common (no-parent) case.
    pub fn create(&self, tool_name: impl Into<String>) -> Arc<RwLock<Job>> {
        self.create_with_parent(tool_name, None)
    }

    /// Create a new job with an optional parent id (issue #318).
    ///
    /// When `parent_job_id` refers to a currently tracked job, the new job's
    /// `cancel_token` is derived from the parent's via
    /// [`CancellationToken::child_token`] — cancelling the parent cancels
    /// this child within one cooperative checkpoint. If the parent id does
    /// not exist the child gets a fresh standalone token and the parent id
    /// is still recorded for diagnostic surfacing.
    pub fn create_with_parent(
        &self,
        tool_name: impl Into<String>,
        parent_job_id: Option<String>,
    ) -> Arc<RwLock<Job>> {
        let cancel_token = match &parent_job_id {
            Some(pid) => match self.jobs.get(pid) {
                Some(parent) => parent.read().cancel_token.child_token(),
                None => CancellationToken::new(),
            },
            None => CancellationToken::new(),
        };
        let job = Job::new(tool_name.into(), parent_job_id, cancel_token);
        let id = job.id.clone();
        let entry = Arc::new(RwLock::new(job));
        self.jobs.insert(id, Arc::clone(&entry));
        {
            let guard = entry.read();
            self.persist_put(&guard);
            self.emit(&guard);
        }
        entry
    }

    /// Transition `Pending → Running`.
    pub fn start(&self, id: &str) -> Option<()> {
        let entry = self.jobs.get(id)?;
        let mut job = entry.write();
        if job.status != JobStatus::Pending {
            debug!(
                job_id = %id,
                from = ?job.status,
                to = ?JobStatus::Running,
                "invalid job transition"
            );
            return None;
        }
        job.status = JobStatus::Running;
        job.updated_at = Utc::now();
        let snapshot = job.clone();
        drop(job);
        self.persist_status(&snapshot.id, snapshot.status, snapshot.updated_at);
        self.emit(&snapshot);
        Some(())
    }

    /// Transition `Running → Completed` and attach a result.
    pub fn complete(&self, id: &str, result: serde_json::Value) -> Option<()> {
        let entry = self.jobs.get(id)?;
        let mut job = entry.write();
        if job.status != JobStatus::Running {
            debug!(
                job_id = %id,
                from = ?job.status,
                to = ?JobStatus::Completed,
                "invalid job transition"
            );
            return None;
        }
        job.status = JobStatus::Completed;
        job.result = Some(result);
        job.updated_at = Utc::now();
        let snapshot = job.clone();
        drop(job);
        self.persist_put(&snapshot);
        self.emit(&snapshot);
        Some(())
    }

    /// Transition `Running → Failed` and attach an error message.
    pub fn fail(&self, id: &str, error: impl Into<String>) -> Option<()> {
        let entry = self.jobs.get(id)?;
        let mut job = entry.write();
        if job.status != JobStatus::Running {
            debug!(
                job_id = %id,
                from = ?job.status,
                to = ?JobStatus::Failed,
                "invalid job transition"
            );
            return None;
        }
        job.status = JobStatus::Failed;
        job.error = Some(error.into());
        job.updated_at = Utc::now();
        let snapshot = job.clone();
        drop(job);
        self.persist_put(&snapshot);
        self.emit(&snapshot);
        Some(())
    }

    /// Cancel a job.  Valid from `Pending` or `Running`; no-op on terminal
    /// states (returns `None`).  Triggers `cancel_token` so the running tool
    /// can observe the cancellation.
    pub fn cancel(&self, id: &str) -> Option<()> {
        let entry = self.jobs.get(id)?;
        let mut job = entry.write();
        match job.status {
            JobStatus::Pending | JobStatus::Running => {
                job.status = JobStatus::Cancelled;
                job.updated_at = Utc::now();
                job.cancel_token.cancel();
                let snapshot = job.clone();
                drop(job);
                self.persist_status(&snapshot.id, snapshot.status, snapshot.updated_at);
                self.emit(&snapshot);
                Some(())
            }
            other => {
                debug!(
                    job_id = %id,
                    from = ?other,
                    to = ?JobStatus::Cancelled,
                    "invalid job transition"
                );
                None
            }
        }
    }

    /// Update the progress of a running job.  Only valid while `Running`.
    pub fn update_progress(&self, id: &str, progress: JobProgress) -> Option<()> {
        let entry = self.jobs.get(id)?;
        let mut job = entry.write();
        if job.status != JobStatus::Running {
            debug!(
                job_id = %id,
                status = ?job.status,
                "ignoring progress update for non-running job"
            );
            return None;
        }
        job.progress = Some(progress);
        job.updated_at = Utc::now();
        let snapshot = job.clone();
        drop(job);
        self.persist_put(&snapshot);
        self.emit(&snapshot);
        Some(())
    }

    /// Get a handle to a job by id.
    pub fn get(&self, id: &str) -> Option<Arc<RwLock<Job>>> {
        self.jobs.get(id).map(|e| Arc::clone(e.value()))
    }

    /// Snapshot of all currently tracked jobs.
    pub fn list(&self) -> Vec<Arc<RwLock<Job>>> {
        self.jobs.iter().map(|e| Arc::clone(e.value())).collect()
    }

    /// Purge terminal jobs whose `updated_at` is older than `older_than`.
    ///
    /// Returns the number of jobs removed.  Non-terminal jobs are never
    /// purged regardless of age.
    pub fn gc_stale(&self, older_than: Duration) -> usize {
        let cutoff = Utc::now() - older_than;
        let stale: Vec<String> = self
            .jobs
            .iter()
            .filter_map(|e| {
                let job = e.value().read();
                if job.status.is_terminal() && job.updated_at < cutoff {
                    Some(job.id.clone())
                } else {
                    None
                }
            })
            .collect();
        let mut removed = 0usize;
        for id in stale {
            if self.jobs.remove(&id).is_some() {
                removed += 1;
            }
        }
        if removed > 0
            && let Some(storage) = &self.storage
            && let Err(e) = storage.delete_older_than(cutoff)
        {
            tracing::warn!(error = %e, "JobStorage.delete_older_than failed during gc_stale");
        }
        removed
    }

    /// TTL-based cleanup used by the built-in `jobs.cleanup` MCP tool
    /// (issue #328). Purges terminal rows older than
    /// `older_than_hours` from both the in-process map and any attached
    /// [`JobStorage`] backend.
    ///
    /// Returns the number of rows removed (from the in-process map —
    /// the storage delete is authoritative for persisted rows).
    pub fn cleanup_older_than_hours(&self, older_than_hours: u64) -> usize {
        // Clamp to i64 to stay inside chrono's range. 1000 years is
        // more than any real caller should ever pass.
        let hours = older_than_hours.min(24 * 365 * 1000) as i64;
        self.gc_stale(Duration::hours(hours))
    }
}
