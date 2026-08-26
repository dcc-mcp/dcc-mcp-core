//! Unit tests for [`crate::job::JobManager`].

use super::persistence::PERSISTENCE_FAILURE_THRESHOLD;
use super::*;
use crate::job_storage::{JobFilter, JobStorage, JobStorageError};
use serde_json::json;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc;
use std::sync::{Arc, Barrier, Condvar};
use std::thread;
use std::time::{Duration, Instant};

#[derive(Debug, Default)]
struct FailingStorage {
    puts: AtomicUsize,
    alternating: bool,
}

impl FailingStorage {
    fn alternating() -> Self {
        Self {
            puts: AtomicUsize::new(0),
            alternating: true,
        }
    }
}

impl JobStorage for FailingStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        let attempt = self.puts.fetch_add(1, Ordering::SeqCst);
        let suffix = if self.alternating { attempt % 2 } else { 0 };
        Err(JobStorageError::Backend(format!("readonly-{suffix}")))
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        Ok(0)
    }
}

struct BlockingStorage {
    entered: mpsc::SyncSender<()>,
    release: std::sync::Mutex<mpsc::Receiver<()>>,
}

impl std::fmt::Debug for BlockingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("BlockingStorage")
            .finish_non_exhaustive()
    }
}

struct NeverReturningStorage {
    entered: mpsc::SyncSender<()>,
}

struct ShutdownObservingStorage {
    put_entered: mpsc::SyncSender<()>,
    put_release: std::sync::Mutex<mpsc::Receiver<()>>,
    delete_called: mpsc::SyncSender<()>,
}

struct FailingDeleteObservingStorage {
    delete_called: mpsc::SyncSender<()>,
}

impl std::fmt::Debug for NeverReturningStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("NeverReturningStorage")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for ShutdownObservingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("ShutdownObservingStorage")
            .finish_non_exhaustive()
    }
}

impl std::fmt::Debug for FailingDeleteObservingStorage {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("FailingDeleteObservingStorage")
            .finish_non_exhaustive()
    }
}

impl JobStorage for FailingDeleteObservingStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        Err(JobStorageError::Backend("readonly".to_string()))
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        self.delete_called.send(()).unwrap();
        Ok(0)
    }
}

impl JobStorage for ShutdownObservingStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        self.put_entered.send(()).unwrap();
        self.put_release.lock().unwrap().recv().unwrap();
        Ok(())
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        self.delete_called.send(()).unwrap();
        Ok(0)
    }
}

impl JobStorage for NeverReturningStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        self.entered.send(()).unwrap();
        loop {
            thread::park();
        }
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        Ok(0)
    }
}

impl JobStorage for BlockingStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        self.entered.send(()).unwrap();
        self.release.lock().unwrap().recv().unwrap();
        Ok(())
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        Ok(0)
    }
}

#[derive(Debug, Default)]
struct ControlledFailingStorage {
    puts: AtomicUsize,
    entered: (std::sync::Mutex<usize>, Condvar),
    releases: (std::sync::Mutex<usize>, Condvar),
}

impl ControlledFailingStorage {
    fn wait_for_puts(&self, expected: usize, timeout: Duration) -> bool {
        let deadline = Instant::now() + timeout;
        let (lock, ready) = &self.entered;
        let mut entered = lock.lock().unwrap();
        while *entered < expected {
            let remaining = deadline.saturating_duration_since(Instant::now());
            if remaining.is_zero() {
                return false;
            }
            let (next, result) = ready.wait_timeout(entered, remaining).unwrap();
            entered = next;
            if result.timed_out() && *entered < expected {
                return false;
            }
        }
        true
    }

    fn release_one(&self) {
        let (lock, ready) = &self.releases;
        *lock.lock().unwrap() += 1;
        ready.notify_one();
    }
}

impl JobStorage for ControlledFailingStorage {
    fn put(&self, _job: &Job) -> Result<(), JobStorageError> {
        self.puts.fetch_add(1, Ordering::SeqCst);
        let (entered_lock, entered_ready) = &self.entered;
        *entered_lock.lock().unwrap() += 1;
        entered_ready.notify_all();

        let (release_lock, release_ready) = &self.releases;
        let mut releases = release_lock.lock().unwrap();
        while *releases == 0 {
            releases = release_ready.wait(releases).unwrap();
        }
        *releases -= 1;
        Err(JobStorageError::Backend("readonly-controlled".to_string()))
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        Ok(0)
    }
}

#[derive(Debug, Default)]
struct RecordingStorage {
    statuses: std::sync::Mutex<Vec<JobStatus>>,
}

impl JobStorage for RecordingStorage {
    fn put(&self, job: &Job) -> Result<(), JobStorageError> {
        self.statuses.lock().unwrap().push(job.status);
        Ok(())
    }

    fn get(&self, _job_id: &str) -> Result<Option<Job>, JobStorageError> {
        Ok(None)
    }

    fn list(&self, _filter: JobFilter) -> Result<Vec<Job>, JobStorageError> {
        Ok(Vec::new())
    }

    fn update_status(
        &self,
        _job_id: &str,
        _status: JobStatus,
        _at: chrono::DateTime<chrono::Utc>,
    ) -> Result<(), JobStorageError> {
        Ok(())
    }

    fn delete_older_than(
        &self,
        _cutoff: chrono::DateTime<chrono::Utc>,
    ) -> Result<u64, JobStorageError> {
        Ok(0)
    }
}

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
fn lifecycle_timestamps_survive_progress_and_completion() {
    let jm = JobManager::new();
    let handle = jm.create("render.sequence");
    let id = handle.read().id.clone();

    assert!(handle.read().started_at.is_none());
    assert!(handle.read().completed_at.is_none());

    jm.start(&id).unwrap();
    let started_at = handle.read().started_at.expect("start timestamp");
    assert!(handle.read().completed_at.is_none());

    jm.update_progress(
        &id,
        JobProgress {
            current: 1,
            total: 2,
            message: Some("halfway".into()),
        },
    )
    .unwrap();
    assert_eq!(handle.read().started_at, Some(started_at));
    assert!(handle.read().completed_at.is_none());

    jm.complete(&id, json!({"ok": true})).unwrap();
    let job = handle.read();
    assert_eq!(job.started_at, Some(started_at));
    assert_eq!(job.completed_at, Some(job.updated_at));
    assert!(job.completed_at >= job.started_at);
}

#[test]
fn cancel_before_start_requires_runner_acknowledgement() {
    let jm = JobManager::new();
    let handle = jm.create("slow.tool");
    let id = handle.read().id.clone();
    let token = handle.read().cancel_token.clone();

    assert!(!token.is_cancelled());
    assert_eq!(jm.cancel(&id), Some(()));
    assert!(token.is_cancelled());
    assert_eq!(handle.read().status, JobStatus::Pending);
    assert_eq!(jm.acknowledge_cancel(&id), Some(()));
    assert_eq!(handle.read().status, JobStatus::Cancelled);
}

#[test]
fn cancel_during_run_requires_runner_acknowledgement() {
    let jm = JobManager::new();
    let handle = jm.create("slow.tool");
    let id = handle.read().id.clone();
    let token = handle.read().cancel_token.clone();

    assert_eq!(jm.start(&id), Some(()));
    assert!(!token.is_cancelled());

    assert_eq!(jm.cancel(&id), Some(()));
    assert!(token.is_cancelled());
    assert_eq!(handle.read().status, JobStatus::Running);
    assert_eq!(jm.acknowledge_cancel(&id), Some(()));
    assert_eq!(handle.read().status, JobStatus::Cancelled);
}

#[test]
fn progress_rejects_regressions_and_invalid_totals() {
    let jm = JobManager::new();
    let handle = jm.create("progress.tool");
    let id = handle.read().id.clone();
    jm.start(&id).unwrap();
    jm.update_progress(
        &id,
        JobProgress {
            current: 2,
            total: 4,
            message: None,
        },
    )
    .unwrap();

    for progress in [
        JobProgress {
            current: 1,
            total: 4,
            message: None,
        },
        JobProgress {
            current: 3,
            total: 3,
            message: None,
        },
        JobProgress {
            current: 5,
            total: 4,
            message: None,
        },
    ] {
        assert_eq!(jm.update_progress(&id, progress), None);
    }
    let progress = handle.read().progress.clone().unwrap();
    assert_eq!((progress.current, progress.total), (2, 4));
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
    assert_eq!(jm.acknowledge_cancel("missing"), None);
}

#[test]
fn chunked_runner_yields_between_steps_and_updates_shared_jobs() {
    let jobs = Arc::new(JobManager::new());
    let pump_work = Arc::new(parking_lot::Mutex::new(Vec::new()));
    let steps = (0..3)
        .map(|index| {
            Box::new(move || {
                Ok(ChunkedStepOutput::new(
                    json!(index),
                    Some(format!("step {index}")),
                ))
            }) as ChunkedStep
        })
        .collect::<Vec<_>>();
    let mut runner = ChunkedJobRunner::new(Arc::clone(&jobs), "maya.render", steps);
    let id = runner.job_id().to_string();

    let mut tick = 0;
    while runner.tick() {
        tick += 1;
        pump_work.lock().push(tick);
        let progress = jobs.get(&id).unwrap().read().progress.clone().unwrap();
        assert_eq!(progress.current, tick);
    }

    assert_eq!(*pump_work.lock(), vec![1, 2]);
    let job = jobs.get(&id).unwrap();
    let job = job.read();
    assert_eq!(job.status, JobStatus::Completed);
    assert_eq!(job.progress.as_ref().unwrap().current, 3);
    assert_eq!(job.result, Some(json!([0, 1, 2])));
}

#[test]
fn chunked_runner_acknowledges_cancel_and_is_host_neutral() {
    for label in ["houdini.cook", "photoshop.export"] {
        let jobs = Arc::new(JobManager::new());
        let ran = Arc::new(std::sync::atomic::AtomicUsize::new(0));
        let steps = (0..2)
            .map(|_| {
                let ran = Arc::clone(&ran);
                Box::new(move || {
                    ran.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
                    Ok(ChunkedStepOutput::new(json!(true), None))
                }) as ChunkedStep
            })
            .collect::<Vec<_>>();
        let mut runner = ChunkedJobRunner::new(Arc::clone(&jobs), label, steps);
        let id = runner.job_id().to_string();

        assert!(runner.tick());
        assert!(runner.cancel());
        assert_eq!(jobs.get(&id).unwrap().read().status, JobStatus::Running);
        assert!(!runner.tick());
        assert_eq!(ran.load(std::sync::atomic::Ordering::SeqCst), 1);
        assert_eq!(jobs.get(&id).unwrap().read().status, JobStatus::Cancelled);
        assert!(!runner.tick());
    }
}

#[test]
fn chunked_runner_publishes_failure_once() {
    let jobs = Arc::new(JobManager::new());
    let steps = vec![Box::new(|| Err("generator failed".to_string())) as ChunkedStep];
    let mut runner = ChunkedJobRunner::new(Arc::clone(&jobs), "blender.bake", steps);
    let id = runner.job_id().to_string();

    assert!(!runner.tick());
    let updated_at = jobs.get(&id).unwrap().read().updated_at;
    assert!(!runner.tick());
    let job = jobs.get(&id).unwrap();
    let job = job.read();
    assert_eq!(job.status, JobStatus::Failed);
    assert_eq!(job.error.as_deref(), Some("generator failed"));
    assert_eq!(job.updated_at, updated_at);
}

#[test]
fn gc_stale_purges_only_terminal_and_old_jobs() {
    let jm = JobManager::new();

    // Terminal + old → purged
    let old_done = jm.create("a");
    let old_done_id = old_done.read().id.clone();
    jm.start(&old_done_id).unwrap();
    jm.complete(&old_done_id, json!(null)).unwrap();
    old_done.write().updated_at = chrono::Utc::now() - chrono::Duration::seconds(120);

    // Terminal but fresh → kept
    let fresh_done = jm.create("b");
    let fresh_done_id = fresh_done.read().id.clone();
    jm.start(&fresh_done_id).unwrap();
    jm.complete(&fresh_done_id, json!(null)).unwrap();

    // Non-terminal but old → kept (non-terminal wins)
    let old_running = jm.create("c");
    let old_running_id = old_running.read().id.clone();
    jm.start(&old_running_id).unwrap();
    old_running.write().updated_at = chrono::Utc::now() - chrono::Duration::seconds(120);

    // Non-terminal and fresh → kept
    let fresh_pending = jm.create("d");
    let fresh_pending_id = fresh_pending.read().id.clone();

    let removed = jm.gc_stale(chrono::Duration::seconds(60));
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

#[test]
fn persistence_status_is_not_configured_without_storage() {
    assert_eq!(
        JobManager::new().persistence_status(),
        JobPersistenceStatus {
            state: JobPersistenceState::NotConfigured,
            consecutive_failures: 0,
            last_error_kind: None,
        }
    );
}

#[test]
fn repeated_identical_put_failures_latch_persistence() {
    let storage = Arc::new(FailingStorage::default());
    let jobs = JobManager::with_storage(storage.clone());

    for _ in 0..5 {
        jobs.create("scene.inspect");
    }

    assert_eq!(storage.puts.load(Ordering::SeqCst), 3);
    assert_eq!(
        jobs.persistence_status(),
        JobPersistenceStatus {
            state: JobPersistenceState::Disabled,
            consecutive_failures: 3,
            last_error_kind: Some("backend".to_string()),
        }
    );
    assert_eq!(jobs.list().len(), 5, "in-memory jobs must keep working");
}

#[test]
fn concurrent_identical_failures_do_not_start_a_fourth_backend_write() {
    let storage = Arc::new(ControlledFailingStorage::default());
    let jobs = Arc::new(JobManager::with_storage(storage.clone()));
    let start = Arc::new(Barrier::new(5));
    let writers: Vec<_> = (0..4)
        .map(|index| {
            let jobs = jobs.clone();
            let start = start.clone();
            thread::spawn(move || {
                start.wait();
                jobs.create(format!("scene.inspect.{index}"));
            })
        })
        .collect();

    start.wait();
    let all_four_started_before_any_result = storage.wait_for_puts(4, Duration::from_millis(500));

    if all_four_started_before_any_result {
        for _ in 0..4 {
            storage.release_one();
        }
    } else {
        assert!(storage.wait_for_puts(1, Duration::from_secs(2)));
        for expected in 1..=3 {
            storage.release_one();
            if expected < 3 {
                assert!(storage.wait_for_puts(expected + 1, Duration::from_secs(2)));
            }
        }
    }

    for writer in writers {
        writer.join().unwrap();
    }

    assert_eq!(
        storage.puts.load(Ordering::SeqCst),
        3,
        "the failure threshold must reserve write ownership before backend I/O"
    );
    assert_eq!(
        jobs.list().len(),
        4,
        "all jobs must remain available in memory"
    );
    assert_eq!(
        jobs.persistence_status().state,
        JobPersistenceState::Disabled
    );
}

#[test]
fn offloaded_storage_flushes_job_lifecycle_in_fifo_order_on_drop() {
    let storage = Arc::new(RecordingStorage::default());
    {
        let jobs = JobManager::with_offloaded_storage(storage.clone());
        let job = jobs.create("scene.inspect");
        let job_id = job.read().id.clone();
        jobs.start(&job_id).unwrap();
        jobs.complete(&job_id, json!({"ok": true})).unwrap();
    }

    assert_eq!(
        *storage.statuses.lock().unwrap(),
        [JobStatus::Pending, JobStatus::Running, JobStatus::Completed,]
    );
}

#[test]
fn offloaded_storage_drop_is_bounded_when_backend_never_returns() {
    const HELPER_ENV: &str = "DCC_MCP_TEST_BLOCKED_PERSISTENCE_DROP_HELPER";
    const READY_PATH_ENV: &str = "DCC_MCP_TEST_BLOCKED_PERSISTENCE_DROP_READY_PATH";
    const TEST_NAME: &str =
        "job::tests::offloaded_storage_drop_is_bounded_when_backend_never_returns";

    if std::env::var_os(HELPER_ENV).is_some() {
        let ready_path = std::path::PathBuf::from(
            std::env::var_os(READY_PATH_ENV).expect("watchdog helper ready path is missing"),
        );
        let (entered_tx, entered_rx) = mpsc::sync_channel(1);
        let jobs = JobManager::with_offloaded_storage(Arc::new(NeverReturningStorage {
            entered: entered_tx,
        }));
        jobs.create("scene.inspect.blocked-drop");
        entered_rx
            .recv_timeout(Duration::from_secs(2))
            .expect("storage worker did not enter the blocking put");
        std::fs::write(ready_path, b"ready").expect("failed to publish watchdog ready marker");
        drop(jobs);
        return;
    }

    let ready_path = std::env::temp_dir().join(format!(
        "dcc-mcp-persistence-drop-{}-{}.ready",
        std::process::id(),
        uuid::Uuid::new_v4()
    ));
    let mut child = std::process::Command::new(std::env::current_exe().unwrap())
        .arg(TEST_NAME)
        .arg("--exact")
        .env(HELPER_ENV, "1")
        .env(READY_PATH_ENV, &ready_path)
        .stdin(std::process::Stdio::null())
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .spawn()
        .expect("failed to spawn persistence-drop watchdog helper");

    let ready_deadline = Instant::now() + Duration::from_secs(5);
    while !ready_path.is_file() {
        if let Some(status) = child.try_wait().unwrap() {
            let _ = std::fs::remove_file(&ready_path);
            panic!("watchdog helper exited before ready with {status}");
        }
        if Instant::now() >= ready_deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            let _ = std::fs::remove_file(&ready_path);
            panic!("watchdog helper did not enter the blocked storage call within 5 seconds");
        }
        thread::sleep(Duration::from_millis(10));
    }

    let deadline = Instant::now() + Duration::from_secs(1);
    loop {
        if let Some(status) = child.try_wait().unwrap() {
            let _ = std::fs::remove_file(&ready_path);
            assert!(status.success(), "watchdog helper exited with {status}");
            break;
        }
        if Instant::now() >= deadline {
            child.kill().unwrap();
            child.wait().unwrap();
            let _ = std::fs::remove_file(&ready_path);
            panic!("dropping a blocked persistence writer did not finish within 1 second");
        }
        thread::sleep(Duration::from_millis(10));
    }
}

#[test]
fn offloaded_storage_shutdown_timeout_cancels_queued_cleanup() {
    let (put_entered_tx, put_entered_rx) = mpsc::sync_channel(1);
    let (put_release_tx, put_release_rx) = mpsc::sync_channel(1);
    let (delete_called_tx, delete_called_rx) = mpsc::sync_channel(1);
    let storage = Arc::new(ShutdownObservingStorage {
        put_entered: put_entered_tx,
        put_release: std::sync::Mutex::new(put_release_rx),
        delete_called: delete_called_tx,
    });
    let jobs = JobManager::with_offloaded_storage(storage);
    let job = jobs.create("scene.inspect.queued-cleanup");
    put_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("storage worker did not enter the blocking put");

    let job_id = job.read().id.clone();
    jobs.start(&job_id).unwrap();
    jobs.complete(&job_id, json!({"ok": true})).unwrap();
    job.write().updated_at = chrono::Utc::now() - chrono::Duration::hours(2);
    assert_eq!(jobs.cleanup_older_than_hours(1), 1);

    let drop_started = Instant::now();
    drop(jobs);
    assert!(
        drop_started.elapsed() < Duration::from_secs(1),
        "manager drop exceeded its bounded shutdown window"
    );

    put_release_tx.send(()).unwrap();
    assert!(
        delete_called_rx
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "a detached disabled worker executed queued cleanup after manager drop"
    );
}

#[test]
fn offloaded_storage_queue_full_disable_cancels_cleanup_before_backend() {
    let (put_entered_tx, put_entered_rx) = mpsc::sync_channel(1);
    let (put_release_tx, put_release_rx) = mpsc::sync_channel(1);
    let (delete_called_tx, delete_called_rx) = mpsc::sync_channel(1);
    let storage = Arc::new(ShutdownObservingStorage {
        put_entered: put_entered_tx,
        put_release: std::sync::Mutex::new(put_release_rx),
        delete_called: delete_called_tx,
    });
    let jobs = JobManager::with_offloaded_storage(storage);
    let job = jobs.create("scene.inspect.queue-full-cleanup");
    put_entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("storage worker did not enter the blocking put");

    let job_id = job.read().id.clone();
    jobs.start(&job_id).unwrap();
    jobs.complete(&job_id, json!({"ok": true})).unwrap();
    job.write().updated_at = chrono::Utc::now() - chrono::Duration::hours(2);
    assert_eq!(jobs.cleanup_older_than_hours(1), 1);
    for index in 0..65 {
        jobs.create(format!("scene.inspect.queue-full-cleanup.{index}"));
    }
    assert_eq!(
        jobs.persistence_status(),
        JobPersistenceStatus {
            state: JobPersistenceState::Disabled,
            consecutive_failures: 0,
            last_error_kind: Some("queue_full".to_string()),
        }
    );

    put_release_tx.send(()).unwrap();
    drop(jobs);
    assert!(
        delete_called_rx
            .recv_timeout(Duration::from_millis(500))
            .is_err(),
        "queue-full disable allowed cleanup to reach the backend"
    );
}

#[test]
fn disabled_storage_circuit_rejects_cleanup_before_enqueue() {
    let (delete_called_tx, delete_called_rx) = mpsc::sync_channel(1);
    let jobs = JobManager::with_storage(Arc::new(FailingDeleteObservingStorage {
        delete_called: delete_called_tx,
    }));
    for index in 0..PERSISTENCE_FAILURE_THRESHOLD {
        jobs.create(format!("scene.inspect.disable-before-cleanup.{index}"));
    }
    assert_eq!(
        jobs.persistence_status().state,
        JobPersistenceState::Disabled
    );

    let old_job = jobs.create("scene.inspect.disabled-cleanup");
    let old_job_id = old_job.read().id.clone();
    jobs.start(&old_job_id).unwrap();
    jobs.complete(&old_job_id, json!({"ok": true})).unwrap();
    old_job.write().updated_at = chrono::Utc::now() - chrono::Duration::hours(2);
    assert_eq!(jobs.cleanup_older_than_hours(1), 1);

    assert!(
        delete_called_rx
            .recv_timeout(Duration::from_millis(100))
            .is_err(),
        "a disabled persistence circuit accepted cleanup backend I/O"
    );
}

#[test]
fn offloaded_storage_queue_is_bounded_and_fails_closed_without_blocking_callers() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let storage = Arc::new(BlockingStorage {
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    let jobs = Arc::new(JobManager::with_offloaded_storage(storage));
    jobs.create("scene.inspect.blocked");
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("storage worker did not start the blocking write");

    let flood_jobs = jobs.clone();
    let (done_tx, done_rx) = mpsc::sync_channel(1);
    let flood = thread::spawn(move || {
        for index in 0..65 {
            flood_jobs.create(format!("scene.inspect.queued.{index}"));
        }
        done_tx.send(()).unwrap();
    });
    done_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("bounded persistence queue blocked a job caller");

    assert_eq!(
        jobs.persistence_status(),
        JobPersistenceStatus {
            state: JobPersistenceState::Disabled,
            consecutive_failures: 0,
            last_error_kind: Some("queue_full".to_string()),
        }
    );
    assert_eq!(jobs.list().len(), 66);

    release_tx.send(()).unwrap();
    flood.join().unwrap();
    drop(jobs);
}

#[test]
fn different_put_failures_do_not_trip_identical_error_latch() {
    let storage = Arc::new(FailingStorage::alternating());
    let jobs = JobManager::with_storage(storage.clone());

    for _ in 0..6 {
        jobs.create("scene.inspect");
    }

    assert_eq!(storage.puts.load(Ordering::SeqCst), 6);
    assert_eq!(
        jobs.persistence_status().state,
        JobPersistenceState::Degraded
    );
    assert_eq!(jobs.persistence_status().consecutive_failures, 1);
}

#[test]
fn persistence_status_does_not_wait_for_a_blocked_storage_write() {
    let (entered_tx, entered_rx) = mpsc::sync_channel(1);
    let (release_tx, release_rx) = mpsc::sync_channel(1);
    let storage = Arc::new(BlockingStorage {
        entered: entered_tx,
        release: std::sync::Mutex::new(release_rx),
    });
    let jobs = Arc::new(JobManager::with_storage(storage));

    let writer_jobs = jobs.clone();
    let writer = thread::spawn(move || writer_jobs.create("scene.inspect"));
    entered_rx
        .recv_timeout(Duration::from_secs(2))
        .expect("storage write did not start");

    let (status_tx, status_rx) = mpsc::sync_channel(1);
    let status_jobs = jobs.clone();
    let status_reader = thread::spawn(move || {
        status_tx.send(status_jobs.persistence_status()).unwrap();
    });
    let status = status_rx.recv_timeout(Duration::from_secs(2));

    release_tx.send(()).unwrap();
    writer.join().unwrap();
    status_reader.join().unwrap();

    let status = status.expect("persistence health blocked behind storage I/O");
    assert_eq!(status.state, JobPersistenceState::Healthy);
}
