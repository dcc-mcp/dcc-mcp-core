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
