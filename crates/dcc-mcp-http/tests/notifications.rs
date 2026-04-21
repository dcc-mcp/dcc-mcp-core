//! Integration tests for [`JobNotifier`] wiring (#326).
//!
//! Covers:
//! - `JobManager` state transitions produce `notifications/progress` frames
//!   when a `progressToken` is registered.
//! - Every transition produces `notifications/$/dcc.jobUpdated` when
//!   `enable_job_notifications` is on.
//! - `emit_workflow_update` synthesises a well-formed
//!   `notifications/$/dcc.workflowUpdated` payload.

use dcc_mcp_http::job::{JobManager, JobStatus};
use dcc_mcp_http::notifications::{JobNotifier, WorkflowProgress, WorkflowUpdate};
use dcc_mcp_http::session::SessionManager;
use dcc_mcp_workflow::WorkflowStatus;
use serde_json::Value;
use tokio::sync::broadcast;

fn drain(rx: &mut broadcast::Receiver<String>) -> Vec<Value> {
    let mut out = Vec::new();
    while let Ok(frame) = rx.try_recv() {
        if let Some(body) = frame.strip_prefix("data: ") {
            if let Ok(v) = serde_json::from_str::<Value>(body.trim()) {
                out.push(v);
            }
        }
    }
    out
}

fn wire(
    enable: bool,
) -> (
    SessionManager,
    String,
    broadcast::Receiver<String>,
    JobNotifier,
    JobManager,
) {
    let sessions = SessionManager::new();
    let sid = sessions.create();
    let rx = sessions.subscribe(&sid).unwrap();
    let notifier = JobNotifier::new(sessions.clone(), enable);
    notifier.subscribe_session(&sid);
    let jm = JobManager::new();
    let cb = notifier.clone();
    jm.subscribe(move |ev| cb.on_job_event(ev));
    (sessions, sid, rx, notifier, jm)
}

#[test]
fn progress_fires_only_when_token_registered() {
    let (_s, sid, mut rx, notifier, jm) = wire(false);
    // No token → channel A silent; also flag off → channel B silent.
    let job = jm.create("t");
    let id = job.read().id.clone();
    notifier.register_job(&id, &sid, None);
    jm.start(&id).unwrap();
    jm.complete(&id, Value::Null).unwrap();

    let events = drain(&mut rx);
    assert!(events.is_empty(), "no events expected, got {events:?}");
}

#[test]
fn progress_token_fires_on_each_transition_even_when_job_updates_disabled() {
    let (_s, sid, mut rx, notifier, jm) = wire(false);
    let job = jm.create("t");
    let id = job.read().id.clone();
    notifier.register_job(&id, &sid, Some(Value::from("tok")));
    jm.start(&id).unwrap();
    jm.complete(&id, Value::Null).unwrap();

    let events = drain(&mut rx);
    let progress: Vec<_> = events
        .iter()
        .filter(|e| e["method"] == "notifications/progress")
        .collect();
    assert!(
        progress.len() >= 2,
        "expected >=2 progress events, got {events:?}"
    );
    assert!(
        events
            .iter()
            .all(|e| e["method"] != "notifications/$/dcc.jobUpdated"),
        "job updates channel leaked while disabled"
    );
}

#[test]
fn job_updates_fire_on_every_transition_with_correct_payload() {
    let (_s, sid, mut rx, notifier, jm) = wire(true);
    let job = jm.create("scene.get_info");
    let id = job.read().id.clone();
    notifier.register_job(&id, &sid, None);
    jm.start(&id).unwrap();
    jm.complete(&id, serde_json::json!({"ok": true})).unwrap();

    let events = drain(&mut rx);
    let updates: Vec<_> = events
        .iter()
        .filter(|e| e["method"] == "notifications/$/dcc.jobUpdated")
        .collect();

    // Pending is emitted by `create` BEFORE `register_job` runs, so we see
    // running + completed (2 frames) on this channel.
    assert!(
        updates.len() >= 2,
        "expected >=2 job updates, got {updates:?}"
    );
    let completed = updates.last().unwrap();
    assert_eq!(completed["params"]["status"], "completed");
    assert_eq!(completed["params"]["tool"], "scene.get_info");
    assert_eq!(completed["params"]["job_id"], id);
    assert!(completed["params"]["started_at"].is_string());
    assert!(completed["params"]["completed_at"].is_string());
}

#[test]
fn failed_job_sets_error_field_on_updated_channel() {
    let (_s, sid, mut rx, notifier, jm) = wire(true);
    let job = jm.create("t");
    let id = job.read().id.clone();
    notifier.register_job(&id, &sid, None);
    jm.start(&id).unwrap();
    jm.fail(&id, "kaboom").unwrap();

    let events = drain(&mut rx);
    let failed = events
        .iter()
        .filter(|e| e["method"] == "notifications/$/dcc.jobUpdated")
        .find(|e| e["params"]["status"] == "failed")
        .expect("failed update missing");
    assert_eq!(failed["params"]["error"], "kaboom");
}

#[test]
fn cancel_emits_cancelled_on_both_channels() {
    let (_s, sid, mut rx, notifier, jm) = wire(true);
    let job = jm.create("t");
    let id = job.read().id.clone();
    notifier.register_job(&id, &sid, Some(Value::from("tok")));
    jm.start(&id).unwrap();
    jm.cancel(&id).unwrap();

    let events = drain(&mut rx);
    let prog_cancelled = events
        .iter()
        .filter(|e| e["method"] == "notifications/progress")
        .find(|e| e["params"]["message"] == "cancelled");
    assert!(
        prog_cancelled.is_some(),
        "progress cancelled missing: {events:?}"
    );
    let upd_cancelled = events
        .iter()
        .filter(|e| e["method"] == "notifications/$/dcc.jobUpdated")
        .find(|e| e["params"]["status"] == "cancelled");
    assert!(
        upd_cancelled.is_some(),
        "$/dcc.jobUpdated cancelled missing"
    );
}

#[test]
fn workflow_update_fires_on_subscribed_sessions() {
    let (sessions, sid, mut rx, notifier, _jm) = wire(true);
    let _ = sid;
    let _ = sessions;
    notifier.emit_workflow_update(WorkflowUpdate {
        workflow_id: uuid::Uuid::nil(),
        job_id: uuid::Uuid::nil(),
        status: WorkflowStatus::Running,
        current_step_id: Some("step-1".into()),
        progress: WorkflowProgress {
            completed_steps: 1,
            total_steps: 3,
        },
    });
    let events = drain(&mut rx);
    let wf = events
        .iter()
        .find(|e| e["method"] == "notifications/$/dcc.workflowUpdated")
        .expect("workflow update missing");
    assert_eq!(wf["params"]["current_step_id"], "step-1");
    assert_eq!(wf["params"]["progress"]["completed_steps"], 1);
    assert_eq!(wf["params"]["progress"]["total_steps"], 3);
    assert_eq!(wf["params"]["status"], "running");
}

#[test]
fn workflow_update_suppressed_when_notifications_disabled() {
    let (_s, _sid, mut rx, notifier, _jm) = wire(false);
    notifier.emit_workflow_update(WorkflowUpdate {
        workflow_id: uuid::Uuid::nil(),
        job_id: uuid::Uuid::nil(),
        status: WorkflowStatus::Completed,
        current_step_id: None,
        progress: WorkflowProgress::default(),
    });
    let events = drain(&mut rx);
    assert!(
        events
            .iter()
            .all(|e| e["method"] != "notifications/$/dcc.workflowUpdated"),
        "workflow channel leaked while disabled"
    );
}

#[test]
fn interrupted_status_maps_to_terminal_message() {
    // Sanity: the progress mapping supports the reserved Interrupted state
    // even though JobManager never transitions to it directly today.
    let s = JobStatus::Interrupted;
    assert!(s.is_terminal());
}
