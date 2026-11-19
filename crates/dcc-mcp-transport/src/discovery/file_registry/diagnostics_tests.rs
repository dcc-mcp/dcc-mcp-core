use super::*;
use fs4::FileExt;
use std::thread;
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
    // Zero-wait acquisitions are encoded as null; writes are still counted.
    assert_eq!(registry.write_diagnostics(), snapshot);
}

#[test]
fn lock_timeout_is_recorded_for_heartbeat_writers() {
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

    drop(holder);
    registry.heartbeat(&maya.key()).unwrap();
    let recovered = diagnostics::snapshot(dir.path());
    assert_eq!(recovered.heartbeat_lock_timeouts, 1);
    assert!(recovered.last_heartbeat_write_ms.is_some());
}

#[test]
fn slow_lock_warn_is_suppressed_until_quiet_transaction() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new_with_lock_policy(
        dir.path(),
        Duration::from_secs(2),
        Duration::from_millis(1),
    )
    .unwrap();

    registry.log_lock_wait(Duration::from_millis(20), "registry lock file");
    registry.log_lock_wait(Duration::from_millis(300), "registry lock file");
    registry.log_lock_wait(Duration::from_millis(400), "registry lock file");
    assert!(registry.lock_wait_episode_open.load(Ordering::Relaxed));

    registry.log_lock_wait(Duration::ZERO, "registry lock file");
    assert!(!registry.lock_wait_episode_open.load(Ordering::Relaxed));
    registry.log_lock_wait(Duration::from_millis(300), "registry lock file");
    assert!(registry.lock_wait_episode_open.load(Ordering::Relaxed));
}
