use super::*;
use fs4::FileExt;
use std::sync::Arc;
use std::thread;
use std::time::UNIX_EPOCH;

fn writer_update(
    is_heartbeat: bool,
    is_registry_lock: bool,
    lock_timeout: bool,
    lock_wait_ms: Option<u64>,
) -> diagnostics::WriterUpdate {
    diagnostics::WriterUpdate {
        is_heartbeat,
        is_registry_lock,
        lock_timeout,
        lock_wait_ms,
        heartbeat_write_ms: None,
    }
}

#[test]
fn write_diagnostics_track_concurrent_heartbeat_writes() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    let registry = Arc::new(FileRegistry::new(dir.path()).unwrap());
    let maya = ServiceEntry::new("maya", "127.0.0.1", 19100);
    let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", 19101);
    let maya_key = maya.key();
    let photoshop_key = photoshop.key();
    registry.register(maya).unwrap();
    registry.register(photoshop).unwrap();

    let writer_a = Arc::clone(&registry);
    let writer_b = Arc::clone(&registry);
    let a = thread::spawn(move || {
        for _ in 0..10 {
            writer_a.heartbeat(&maya_key).unwrap();
        }
    });
    let b = thread::spawn(move || {
        for _ in 0..10 {
            writer_b.heartbeat(&photoshop_key).unwrap();
        }
    });
    a.join().unwrap();
    b.join().unwrap();

    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 0);
    assert!(snapshot.last_heartbeat_write_ms.is_some());
    assert_eq!(registry.write_diagnostics(), snapshot);
}

#[test]
fn lock_timeout_is_recorded_only_for_heartbeat_writers() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    let registry = FileRegistry::new_with_lock_policy(
        dir.path(),
        Duration::from_millis(10),
        Duration::from_millis(1),
    )
    .unwrap();
    let maya = ServiceEntry::new("maya", "127.0.0.1", 19110);
    registry.register(maya.clone()).unwrap();
    let holder = File::options()
        .read(true)
        .write(true)
        .open(dir.path().join(REGISTRY_LOCK_FILE))
        .unwrap();
    assert!(matches!(FileExt::try_lock(&holder), Ok(())));

    let timeout_error = registry.heartbeat(&maya.key()).unwrap_err().to_string();
    assert!(timeout_error.contains("registry lock file"));
    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 1);
    assert_eq!(snapshot.other_write_lock_timeouts, 0);
    assert!(snapshot.registry_lock_wait_ms.is_some());

    let maya_key = maya.key();
    let _ = registry.update_status(&maya_key, ServiceStatus::Busy);
    let after_other = diagnostics::snapshot(dir.path());
    assert_eq!(after_other.heartbeat_lock_timeouts, 1);
    assert_eq!(after_other.other_write_lock_timeouts, 1);

    drop(holder);
    registry.heartbeat(&maya.key()).unwrap();
    let recovered = diagnostics::snapshot(dir.path());
    assert_eq!(recovered.heartbeat_lock_timeouts, 1);
    assert_eq!(recovered.other_write_lock_timeouts, 1);
    assert!(recovered.last_heartbeat_write_ms.is_some());
}

#[test]
fn timeout_warning_episode_is_stable_across_handles() {
    let dir = tempfile::tempdir().unwrap();
    let diagnostics_dir = dir.path().join("registry-write-diagnostics");
    let record_path = diagnostics_dir.join(format!(
        "v{}-writer-{}.json",
        diagnostics::DIAGNOSTICS_SCHEMA_VERSION,
        std::process::id()
    ));
    fs::create_dir_all(&diagnostics_dir).unwrap();
    let process_start_ms = diagnostics::process_identity_for_current_process();
    fs::write(
        &record_path,
        serde_json::json!({
            "schema_version": diagnostics::DIAGNOSTICS_SCHEMA_VERSION,
            "pid": std::process::id(),
            "process_start_ms": process_start_ms,
            "started_at_unix_ms": 0,
            "updated_at_unix_ms": 0,
            "counters": {
                "heartbeat_lock_timeouts": 0,
                "last_heartbeat_write_ms": 0,
                "registry_lock_wait_ms": 0,
                "other_write_lock_timeouts": 0,
            },
            "warning_episode_open": true
        })
        .to_string(),
    )
    .unwrap();

    let first = FileRegistry::new_with_lock_policy(
        dir.path(),
        Duration::from_millis(10),
        Duration::from_millis(1),
    )
    .unwrap();
    let second = FileRegistry::new_with_lock_policy(
        dir.path(),
        Duration::from_millis(10),
        Duration::from_millis(1),
    )
    .unwrap();

    let first_decision = first
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(10)));
    assert_eq!(first_decision, diagnostics::WarningDecision::Suppress);

    let second_decision = second
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(10)));
    assert_eq!(second_decision, diagnostics::WarningDecision::Suppress);

    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 2);
}

#[test]
fn quiet_wait_closes_warning_episode_for_next_handle() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    let first = FileRegistry::new(dir.path()).unwrap();
    let second = FileRegistry::new(dir.path()).unwrap();

    let noisy = first
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(300)));
    assert_eq!(noisy, diagnostics::WarningDecision::Emit);
    let suppressed = second
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(300)));
    assert_eq!(suppressed, diagnostics::WarningDecision::Suppress);

    first
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, false, Some(5)));
    let emitted_again = second
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(300)));
    assert_eq!(emitted_again, diagnostics::WarningDecision::Emit);
}

#[test]
fn in_process_mutex_waits_do_not_inflate_file_lock_timeouts() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    let registry = Arc::new(
        FileRegistry::new_with_lock_policy(
            dir.path(),
            Duration::from_millis(10),
            Duration::from_millis(1),
        )
        .unwrap(),
    );
    let maya = ServiceEntry::new("maya", "127.0.0.1", 19130);
    registry.register(maya.clone()).unwrap();
    let _guard = registry.write_lock.lock().unwrap();
    let error = registry.heartbeat(&maya.key()).unwrap_err();
    assert!(error.to_string().contains("in-process registry mutex"));
    drop(_guard);

    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 0);
    assert_eq!(snapshot.other_write_lock_timeouts, 0);
}

#[test]
fn active_writer_is_not_cleaned_up_after_old_start_time() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    FileRegistry::new(dir.path()).unwrap();
    let process_start_ms = diagnostics::process_start_ms(dir.path());
    let stale_age = diagnostics::MAX_STALE_AGE.as_millis() as u64 + 1;
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    diagnostics::write_test_record(
        dir.path(),
        now.saturating_sub(stale_age),
        now,
        process_start_ms,
    );
    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 7);

    diagnostics::write_test_record(
        dir.path(),
        now.saturating_sub(stale_age),
        now.saturating_sub(stale_age),
        process_start_ms,
    );
    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 0);
}

#[test]
fn stale_writer_start_token_still_bypasses_age_check() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    FileRegistry::new(dir.path()).unwrap();
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as u64;

    diagnostics::write_test_record(dir.path(), now, now, Some(u64::MAX));
    assert_eq!(diagnostics::snapshot(dir.path()).heartbeat_lock_timeouts, 0);
}

#[test]
fn concurrent_handle_creation_preserves_counters_and_episode() {
    let dir = tempfile::tempdir().unwrap();
    diagnostics::reset_counts(dir.path());
    let first = FileRegistry::new(dir.path()).unwrap();
    first
        .write_diagnostics
        .lock()
        .unwrap()
        .record(writer_update(true, true, true, Some(10)));

    let handles = Arc::new(Mutex::new(Vec::new()));
    let mut workers = Vec::new();
    for _ in 0..8 {
        let dir = Arc::new(dir.path().to_path_buf());
        let handles = Arc::clone(&handles);
        workers.push(thread::spawn(move || {
            let handle = FileRegistry::new(dir.as_path()).unwrap();
            handles.lock().unwrap().push(handle);
        }));
    }
    for worker in workers {
        worker.join().unwrap();
    }

    for handle in handles.lock().unwrap().iter() {
        handle
            .write_diagnostics
            .lock()
            .unwrap()
            .record(writer_update(true, true, true, Some(10)));
    }

    let snapshot = diagnostics::snapshot(dir.path());
    assert_eq!(snapshot.heartbeat_lock_timeouts, 9);
}
