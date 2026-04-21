//! In-process async job tracker for MCP tool calls (issue #316).
//!
//! `JobManager` provides a lightweight, thread-safe registry for async
//! tool-call lifecycles.  It is used by the MCP HTTP server to expose job
//! status / progress / cancellation to clients without losing state when
//! `handle_tools_call` returns.
//!
//! This module is intentionally pure-Rust.  Python bindings are deferred to
//! issue #319 where a coherent user-facing API (`jobs.get_status`,
//! `jobs.cancel`, …) lands together.
//!
//! # Concurrency model
//!
//! - Jobs are stored in `DashMap<String, Arc<RwLock<Job>>>` — per-entry locks
//!   keep contention local to a single job.
//! - `parking_lot::RwLock` is used instead of `std::sync::RwLock` for
//!   performance and consistency with the rest of the workspace.
//! - `cancel_token` is a `tokio_util::sync::CancellationToken` so long-running
//!   async tool handlers can observe cancellation via `.cancelled().await`.
//!
//! # State machine
//!
//! ```text
//! Pending ──► Running ──► Completed
//!    │           │        ╰► Failed
//!    │           │        ╰► Cancelled
//!    │           ╰► Cancelled
//!    ╰► Cancelled
//! ```
//!
//! Invalid transitions (e.g. `Completed → Running`) are rejected: the mutator
//! returns `None` and logs at `debug` level.  The stored job state is left
//! unchanged.

use std::sync::Arc;

use chrono::{DateTime, Duration, Utc};
use dashmap::DashMap;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use tokio_util::sync::CancellationToken;
use tracing::debug;
use uuid::Uuid;

/// Event emitted whenever a job transitions between [`JobStatus`] values.
///
/// Subscribers receive a snapshot of the job **after** the transition.
/// The transport layer ([`crate::notifications::JobNotifier`]) converts
/// these events into MCP `notifications/progress` and the
/// `notifications/$/dcc.jobUpdated` vendor-extension channel (#326).
#[derive(Debug, Clone)]
pub struct JobEvent {
    /// Job id.
    pub id: String,
    /// Fully-qualified tool name (e.g. `scene.get_info`).
    pub tool_name: String,
    /// New status after the transition.
    pub status: JobStatus,
    /// Last-known progress (may be `None` for `Pending`).
    pub progress: Option<JobProgress>,
    /// Error message attached when `status == Failed`.
    pub error: Option<String>,
    /// Wall-clock time of the transition.
    pub updated_at: DateTime<Utc>,
    /// Wall-clock time when the job was created.
    pub created_at: DateTime<Utc>,
}

/// Boxed subscriber callback. See [`JobManager::subscribe`].
pub type JobSubscriber = Arc<dyn Fn(JobEvent) + Send + Sync + 'static>;

// ── Types ─────────────────────────────────────────────────────────────────

/// Lifecycle state of a tracked [`Job`].
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum JobStatus {
    /// Created but not yet picked up for execution.
    Pending,
    /// Currently executing.
    Running,
    /// Finished successfully.
    Completed,
    /// Finished with an error.
    Failed,
    /// Cancelled by a client or supervisor.
    Cancelled,
    /// Reserved for crash / restart recovery (issue #328).
    Interrupted,
}

impl JobStatus {
    /// Returns `true` if the job is in a terminal state.
    pub fn is_terminal(self) -> bool {
        matches!(
            self,
            JobStatus::Completed
                | JobStatus::Failed
                | JobStatus::Cancelled
                | JobStatus::Interrupted
        )
    }
}

/// Coarse progress indicator emitted by the tool handler.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct JobProgress {
    pub current: u64,
    pub total: u64,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub message: Option<String>,
}

/// A tracked async tool invocation.
#[derive(Debug, Clone, Serialize)]
pub struct Job {
    pub id: String,
    pub tool_name: String,
    pub status: JobStatus,
    /// Parent job id (issue #318 — workflow nesting / cascading cancel).
    ///
    /// When set, this job is a child of another tracked job. The child's
    /// `cancel_token` is derived via [`CancellationToken::child_token`] from
    /// the parent's, so cancelling the parent cancels every descendant
    /// within one cooperative checkpoint.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub parent_job_id: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub progress: Option<JobProgress>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub result: Option<serde_json::Value>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub error: Option<String>,
    pub created_at: DateTime<Utc>,
    pub updated_at: DateTime<Utc>,
    /// Cooperative cancellation signal for the running tool.
    ///
    /// Not serialised — clients observe cancellation through `status`.
    /// For child jobs this is a child token of the parent's, so parent
    /// cancellation propagates automatically.
    #[serde(skip)]
    pub cancel_token: CancellationToken,
}

impl Job {
    fn new(
        tool_name: String,
        parent_job_id: Option<String>,
        cancel_token: CancellationToken,
    ) -> Self {
        let now = Utc::now();
        Self {
            id: Uuid::new_v4().to_string(),
            tool_name,
            status: JobStatus::Pending,
            parent_job_id,
            progress: None,
            result: None,
            error: None,
            created_at: now,
            updated_at: now,
            cancel_token,
        }
    }

    /// JSON status snapshot used by `jobs.get_status` (#319) and the async
    /// dispatch envelope returned by `tools/call` (#318).
    pub fn to_status_json(&self) -> serde_json::Value {
        serde_json::json!({
            "job_id": self.id,
            "tool_name": self.tool_name,
            "status": self.status,
            "parent_job_id": self.parent_job_id,
            "progress": self.progress,
            "result": self.result,
            "error": self.error,
            "created_at": self.created_at,
            "updated_at": self.updated_at,
        })
    }
}

// ── Manager ───────────────────────────────────────────────────────────────

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

// ── Tests ─────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;
    use std::thread;

    #[test]
    fn full_lifecycle_create_start_progress_complete_get() {
        let jm = JobManager::new();
        let handle = jm.create("scene.get_info");
        let id = handle.read().id.clone();

        assert_eq!(handle.read().status, JobStatus::Pending);

        assert_eq!(jm.start(&id), Some(()));
        assert_eq!(handle.read().status, JobStatus::Running);

        assert_eq!(
            jm.update_progress(
                &id,
                JobProgress {
                    current: 1,
                    total: 3,
                    message: Some("loading".into()),
                }
            ),
            Some(())
        );
        assert_eq!(handle.read().progress.as_ref().unwrap().current, 1);

        assert_eq!(jm.complete(&id, json!({"ok": true})), Some(()));
        let job = jm.get(&id).expect("job exists");
        let job = job.read();
        assert_eq!(job.status, JobStatus::Completed);
        assert_eq!(job.result.as_ref().unwrap(), &json!({"ok": true}));
    }

    #[test]
    fn cancel_before_start_marks_cancelled_and_triggers_token() {
        let jm = JobManager::new();
        let handle = jm.create("slow.tool");
        let id = handle.read().id.clone();
        let token = handle.read().cancel_token.clone();

        assert!(!token.is_cancelled());
        assert_eq!(jm.cancel(&id), Some(()));
        assert!(token.is_cancelled());
        assert_eq!(handle.read().status, JobStatus::Cancelled);

        // cannot start a cancelled job
        assert_eq!(jm.start(&id), None);
    }

    #[test]
    fn cancel_during_run_marks_cancelled_and_triggers_token() {
        let jm = JobManager::new();
        let handle = jm.create("slow.tool");
        let id = handle.read().id.clone();
        let token = handle.read().cancel_token.clone();

        assert_eq!(jm.start(&id), Some(()));
        assert!(!token.is_cancelled());

        assert_eq!(jm.cancel(&id), Some(()));
        assert!(token.is_cancelled());
        assert_eq!(handle.read().status, JobStatus::Cancelled);
    }

    #[test]
    fn invalid_transition_returns_none_does_not_panic() {
        let jm = JobManager::new();
        let handle = jm.create("tool");
        let id = handle.read().id.clone();

        assert_eq!(jm.start(&id), Some(()));
        assert_eq!(jm.complete(&id, json!(null)), Some(()));

        // Completed → Running should be rejected
        assert_eq!(jm.start(&id), None);
        // Completed → Failed should be rejected
        assert_eq!(jm.fail(&id, "nope"), None);
        // Completed → Cancelled should be rejected
        assert_eq!(jm.cancel(&id), None);
        // progress on non-running should be rejected
        assert_eq!(
            jm.update_progress(
                &id,
                JobProgress {
                    current: 0,
                    total: 0,
                    message: None
                }
            ),
            None
        );

        assert_eq!(handle.read().status, JobStatus::Completed);
    }

    #[test]
    fn get_and_fail_missing_job_returns_none() {
        let jm = JobManager::new();
        assert!(jm.get("missing").is_none());
        assert_eq!(jm.start("missing"), None);
        assert_eq!(jm.complete("missing", json!(null)), None);
        assert_eq!(jm.fail("missing", "err"), None);
        assert_eq!(jm.cancel("missing"), None);
    }

    #[test]
    fn gc_stale_purges_only_terminal_and_old_jobs() {
        let jm = JobManager::new();

        // Terminal + old → purged
        let old_done = jm.create("a");
        let old_done_id = old_done.read().id.clone();
        jm.start(&old_done_id).unwrap();
        jm.complete(&old_done_id, json!(null)).unwrap();
        old_done.write().updated_at = Utc::now() - Duration::seconds(120);

        // Terminal but fresh → kept
        let fresh_done = jm.create("b");
        let fresh_done_id = fresh_done.read().id.clone();
        jm.start(&fresh_done_id).unwrap();
        jm.complete(&fresh_done_id, json!(null)).unwrap();

        // Non-terminal but old → kept (non-terminal wins)
        let old_running = jm.create("c");
        let old_running_id = old_running.read().id.clone();
        jm.start(&old_running_id).unwrap();
        old_running.write().updated_at = Utc::now() - Duration::seconds(120);

        // Non-terminal and fresh → kept
        let fresh_pending = jm.create("d");
        let fresh_pending_id = fresh_pending.read().id.clone();

        let removed = jm.gc_stale(Duration::seconds(60));
        assert_eq!(removed, 1);

        assert!(jm.get(&old_done_id).is_none());
        assert!(jm.get(&fresh_done_id).is_some());
        assert!(jm.get(&old_running_id).is_some());
        assert!(jm.get(&fresh_pending_id).is_some());
    }

    #[test]
    fn concurrent_create_no_duplicates_no_deadlock() {
        let jm = Arc::new(JobManager::new());
        let n_threads = 100usize;
        let per_thread = 10usize;

        let handles: Vec<_> = (0..n_threads)
            .map(|t| {
                let jm = Arc::clone(&jm);
                thread::spawn(move || {
                    let mut ids = Vec::with_capacity(per_thread);
                    for i in 0..per_thread {
                        let h = jm.create(format!("tool-{t}-{i}"));
                        ids.push(h.read().id.clone());
                    }
                    ids
                })
            })
            .collect();

        let mut all_ids = Vec::with_capacity(n_threads * per_thread);
        for h in handles {
            all_ids.extend(h.join().expect("thread panicked"));
        }

        assert_eq!(all_ids.len(), n_threads * per_thread);
        assert_eq!(jm.list().len(), n_threads * per_thread);

        // no duplicate UUIDs
        let mut sorted = all_ids.clone();
        sorted.sort();
        sorted.dedup();
        assert_eq!(sorted.len(), all_ids.len());
    }

    #[test]
    fn job_status_is_terminal_correct() {
        assert!(!JobStatus::Pending.is_terminal());
        assert!(!JobStatus::Running.is_terminal());
        assert!(JobStatus::Completed.is_terminal());
        assert!(JobStatus::Failed.is_terminal());
        assert!(JobStatus::Cancelled.is_terminal());
        assert!(JobStatus::Interrupted.is_terminal());
    }

    #[test]
    fn serde_status_lowercase() {
        assert_eq!(
            serde_json::to_string(&JobStatus::Running).unwrap(),
            "\"running\""
        );
        let s: JobStatus = serde_json::from_str("\"completed\"").unwrap();
        assert_eq!(s, JobStatus::Completed);
    }
}
