use std::sync::{Arc, Mutex};

use dcc_mcp_job::job::{JobManager, JobProgress, JobStatus};
use serde_json::json;

#[test]
fn native_lifecycle_emits_ordered_terminal_snapshots() {
    let manager = JobManager::new();
    let observed = Arc::new(Mutex::new(Vec::new()));
    let events = observed.clone();
    manager.subscribe(move |event| events.lock().unwrap().push(event));

    let job = manager.create("maya__bake_simulation");
    let id = job.read().id.clone();
    assert_eq!(job.read().status, JobStatus::Pending);

    manager.start(&id).unwrap();
    manager
        .update_progress(
            &id,
            JobProgress {
                current: 24,
                total: 48,
                message: Some("baking".to_string()),
            },
        )
        .unwrap();
    manager.complete(&id, json!({"frames": 48})).unwrap();

    let snapshot = manager.get(&id).unwrap();
    let snapshot = snapshot.read();
    assert_eq!(snapshot.status, JobStatus::Completed);
    assert_eq!(snapshot.result, Some(json!({"frames": 48})));
    assert!(snapshot.completed_at.is_some());

    let statuses: Vec<_> = observed
        .lock()
        .unwrap()
        .iter()
        .map(|event| event.status)
        .collect();
    assert_eq!(
        statuses,
        [
            JobStatus::Pending,
            JobStatus::Running,
            JobStatus::Running,
            JobStatus::Completed,
        ]
    );
}

#[test]
fn parent_cancellation_reaches_cross_host_child() {
    let manager = JobManager::new();
    let parent = manager.create("photoshop__export_document");
    let parent_id = parent.read().id.clone();
    let child =
        manager.create_with_parent("blender__convert_exported_asset", Some(parent_id.clone()));

    manager.cancel(&parent_id).unwrap();

    assert!(parent.read().cancel_token.is_cancelled());
    assert!(child.read().cancel_token.is_cancelled());
}
