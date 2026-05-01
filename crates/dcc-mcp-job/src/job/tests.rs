//! Unit tests for [`crate::job::JobManager`].

use super::*;
use serde_json::json;
use std::sync::Arc;
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
