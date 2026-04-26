//! Integration tests for the async-dispatch cancel-cascade path (#318).
//!
//! Verifies that cancelling a parent job propagates to every child job
//! whose `cancel_token` was derived via `JobManager::create_with_parent`.
//! The child must observe cancellation within one cooperative checkpoint
//! — the acceptance criterion from #318 is "within 100 ms".

use dcc_mcp_http::JobManager;
use std::sync::Arc;
use std::time::{Duration, Instant};

#[tokio::test]
async fn cancelling_parent_cascades_to_child_within_100ms() {
    let jm = Arc::new(JobManager::new());

    let parent = jm.create("workflow.run");
    let parent_id = parent.read().id.clone();

    let child = jm.create_with_parent("workflow.step", Some(parent_id.clone()));
    let (child_id, child_token) = {
        let c = child.read();
        (c.id.clone(), c.cancel_token.clone())
    };

    assert!(!child_token.is_cancelled());

    // Trigger cancel on the parent; the child's token is a child-token of
    // the parent's so it fires automatically.
    jm.start(&parent_id).unwrap();
    let start = Instant::now();
    jm.cancel(&parent_id).expect("parent cancellable");

    let observed = tokio::time::timeout(Duration::from_millis(100), child_token.cancelled())
        .await
        .is_ok();
    let elapsed = start.elapsed();
    assert!(
        observed,
        "child token did not observe parent cancel within 100 ms (elapsed {elapsed:?})"
    );
    assert!(
        elapsed < Duration::from_millis(100),
        "cascade took {elapsed:?}"
    );

    // The child's Job status stays `Pending` until the dispatch loop
    // transitions it — the token cascade alone does not mutate the Job.
    // This is by design: `JobManager::cancel` drives the status transition.
    // Callers that want the child marked `Cancelled` should invoke
    // `jm.cancel(&child_id)` themselves once they observe the token.
    assert!(jm.get(&child_id).is_some(), "child job is still tracked");
}

#[tokio::test]
async fn child_token_fires_even_when_parent_already_started() {
    let jm = Arc::new(JobManager::new());

    let parent = jm.create("workflow.run");
    let parent_id = parent.read().id.clone();
    jm.start(&parent_id).unwrap();

    // Create the child AFTER the parent is running — child_token should
    // still be wired up to the parent's CancellationToken.
    let child = jm.create_with_parent("workflow.step", Some(parent_id.clone()));
    let child_token = child.read().cancel_token.clone();

    jm.cancel(&parent_id).unwrap();
    let observed = tokio::time::timeout(Duration::from_millis(100), child_token.cancelled())
        .await
        .is_ok();
    assert!(observed, "child failed to observe mid-run parent cancel");
}

#[tokio::test]
async fn missing_parent_gets_standalone_token() {
    let jm = Arc::new(JobManager::new());
    // Create a child whose parent id does not exist — the token should be
    // fresh and standalone so the job is still operable.
    let orphan = jm.create_with_parent("workflow.step", Some("nonexistent".into()));
    let token = orphan.read().cancel_token.clone();
    assert!(!token.is_cancelled());
    assert_eq!(
        orphan.read().parent_job_id.as_deref(),
        Some("nonexistent"),
        "parent id recorded for diagnostics even when unresolved"
    );
}

#[test]
fn to_status_json_surfaces_parent_job_id() {
    let jm = JobManager::new();
    let parent = jm.create("workflow.run");
    let parent_id = parent.read().id.clone();
    let child = jm.create_with_parent("workflow.step", Some(parent_id.clone()));
    let json = child.read().to_status_json();
    assert_eq!(
        json.get("parent_job_id").and_then(|v| v.as_str()),
        Some(parent_id.as_str())
    );
    assert_eq!(json.get("status").and_then(|v| v.as_str()), Some("pending"));
    assert!(json.get("job_id").and_then(|v| v.as_str()).is_some());
}
