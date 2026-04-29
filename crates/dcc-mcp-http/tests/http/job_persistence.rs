//! Integration tests for the optional SQLite `JobStorage` backend
//! (issue #328).
//!
//! These tests are compiled only when the `job-persist-sqlite` feature
//! is enabled; the default build silently skips them so CI matrices
//! that do not opt in stay green.

#![cfg(feature = "job-persist-sqlite")]

use std::sync::Arc;

use dcc_mcp_actions::ActionRegistry;
use dcc_mcp_http::config::JobRecoveryPolicy;
use dcc_mcp_http::job::{JobManager, JobStatus};
use dcc_mcp_http::job_storage::{JobFilter, JobStorage, SqliteStorage};
use dcc_mcp_http::{McpHttpConfig, McpHttpServer};
use serde_json::json;
use tempfile::tempdir;

fn open_store(path: &std::path::Path) -> Arc<SqliteStorage> {
    Arc::new(SqliteStorage::open(path).expect("open SQLite storage"))
}

#[test]
fn restart_flips_inflight_jobs_to_interrupted() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("jobs.sqlite3");

    // First "incarnation": create one pending, one running, one terminal row.
    let pending_id;
    let running_id;
    let done_id;
    {
        let storage = open_store(&db);
        let mgr = JobManager::with_storage(storage.clone());

        let pending = mgr.create("pending.tool");
        pending_id = pending.read().id.clone();

        let running = mgr.create("running.tool");
        running_id = running.read().id.clone();
        mgr.start(&running_id).unwrap();

        let done = mgr.create("done.tool");
        done_id = done.read().id.clone();
        mgr.start(&done_id).unwrap();
        mgr.complete(&done_id, json!({"ok": true})).unwrap();

        // Storage should now contain three rows with the expected statuses.
        let all = storage.list(JobFilter::default()).unwrap();
        assert_eq!(all.len(), 3);
    }

    // Second incarnation: open the same file and recover.
    {
        let storage = open_store(&db);
        let mgr = JobManager::with_storage(storage.clone());
        let flipped = mgr.recover_from_storage().unwrap();
        assert_eq!(flipped, 2, "pending + running should both flip");

        // Pending → Interrupted.
        let row = mgr.get(&pending_id).unwrap();
        let row = row.read();
        assert_eq!(row.status, JobStatus::Interrupted);
        assert_eq!(row.error.as_deref(), Some("server restart"));
        drop(row);

        // Running → Interrupted.
        let row = mgr.get(&running_id).unwrap();
        let row = row.read();
        assert_eq!(row.status, JobStatus::Interrupted);
        drop(row);

        // Completed stays Completed.
        let row = mgr.get(&done_id).unwrap();
        let row = row.read();
        assert_eq!(row.status, JobStatus::Completed);
    }

    // Third incarnation: re-recovering must be idempotent — nothing is
    // in-flight anymore so `flipped` should be zero.
    {
        let storage = open_store(&db);
        let mgr = JobManager::with_storage(storage);
        let flipped = mgr.recover_from_storage().unwrap();
        assert_eq!(flipped, 0);
    }
}

#[test]
fn cleanup_older_than_hours_prunes_storage_rows() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("jobs.sqlite3");
    let storage = open_store(&db);
    let mgr = JobManager::with_storage(storage.clone());

    // A completed job backdated 48 hours.
    let done = mgr.create("old.done");
    let done_id = done.read().id.clone();
    mgr.start(&done_id).unwrap();
    mgr.complete(&done_id, json!(null)).unwrap();
    {
        let mut j = done.write();
        j.updated_at = chrono::Utc::now() - chrono::Duration::hours(48);
    }
    storage.put(&done.read()).unwrap();

    // A fresh running job — must NOT be pruned.
    let running = mgr.create("fresh.running");
    let running_id = running.read().id.clone();
    mgr.start(&running_id).unwrap();

    let removed = mgr.cleanup_older_than_hours(24);
    assert_eq!(removed, 1);
    assert!(mgr.get(&done_id).is_none());
    assert!(mgr.get(&running_id).is_some());
    assert!(storage.get(&done_id).unwrap().is_none());
    assert!(storage.get(&running_id).unwrap().is_some());
}

// ── Issue #567: job_recovery policy at server startup ────────────────────

/// Default policy (`Drop`) flips an in-flight row to `Interrupted` on
/// `McpHttpServer::start()`. This locks in the existing behaviour so a
/// future tweak that renames or re-routes the policy switch can't
/// silently regress it.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_start_with_drop_policy_marks_inflight_interrupted() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("jobs.sqlite3");
    let running_id = seed_running_row(&db);

    let cfg = McpHttpConfig::new(0)
        .with_name("recovery-drop-test")
        .with_job_storage_path(&db);
    assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Drop, "default policy");

    let registry = Arc::new(ActionRegistry::new());
    let handle = McpHttpServer::new(registry, cfg)
        .start()
        .await
        .expect("server must start");
    handle.shutdown().await;

    assert_recovered_interrupted(&db, &running_id);
}

/// `Requeue` is accepted but degrades to `Drop` until tool-arg
/// persistence lands. The server MUST still start cleanly and the row
/// MUST end up `Interrupted` — the contract is "behaves like Drop
/// today, real requeue lands in a future release". A regression here
/// would re-introduce the dcc-mcp-maya#567 doc/impl drift.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn server_start_with_requeue_policy_degrades_to_drop_today() {
    let dir = tempdir().unwrap();
    let db = dir.path().join("jobs.sqlite3");
    let running_id = seed_running_row(&db);

    let cfg = McpHttpConfig::new(0)
        .with_name("recovery-requeue-test")
        .with_job_storage_path(&db)
        .with_job_recovery(JobRecoveryPolicy::Requeue);
    assert_eq!(cfg.job_recovery, JobRecoveryPolicy::Requeue);
    assert_eq!(cfg.job_recovery.as_str(), "requeue");

    let registry = Arc::new(ActionRegistry::new());
    let handle = McpHttpServer::new(registry, cfg)
        .start()
        .await
        .expect("requeue policy must NOT block server startup");
    handle.shutdown().await;

    // Until real requeue lands the row reaches the same terminal state
    // it would under Drop. The accompanying `WARN` log is intentional —
    // we don't assert on its text here to avoid a tracing-subscriber
    // dependency, but the contract is exercised by the second-run
    // assertion below.
    assert_recovered_interrupted(&db, &running_id);
}

fn seed_running_row(db: &std::path::Path) -> String {
    let storage = open_store(db);
    let mgr = JobManager::with_storage(storage);
    let job = mgr.create("scene.export");
    let id = job.read().id.clone();
    mgr.start(&id).expect("transition pending -> running");
    id
}

fn assert_recovered_interrupted(db: &std::path::Path, job_id: &str) {
    let storage = open_store(db);
    let row = storage
        .get(job_id)
        .expect("storage get")
        .expect("row exists");
    assert_eq!(row.status, JobStatus::Interrupted);
    assert_eq!(row.error.as_deref(), Some("server restart"));
}
