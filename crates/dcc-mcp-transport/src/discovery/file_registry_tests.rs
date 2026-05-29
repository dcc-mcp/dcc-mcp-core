//! Unit tests for `FileRegistry`.

use super::*;
use uuid::Uuid;

fn remove_sentinel_for_pid_fallback(registry: &FileRegistry, key: &ServiceKey) {
    registry.sentinel_handles.remove(key);
    if let Some(mut entry) = registry.services.get_mut(key) {
        entry.sentinel_path = None;
    }
}

fn corrupted_registry_files(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut paths: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .filter(|path| {
            path.file_name()
                .and_then(|name| name.to_str())
                .is_some_and(|name| name.starts_with("services.json.corrupted-"))
        })
        .collect();
    paths.sort();
    paths
}

fn temp_registry_file_names(dir: &std::path::Path) -> Vec<String> {
    let mut names: Vec<_> = std::fs::read_dir(dir)
        .unwrap()
        .filter_map(Result::ok)
        .map(|entry| entry.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp."))
        .collect();
    names.sort();
    names
}

#[test]
fn test_file_registry_register_and_list() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry1 = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let entry2 = ServiceEntry::new("maya", "127.0.0.1", 18813);
    let entry3 = ServiceEntry::new("blender", "127.0.0.1", 9090);

    registry.register(entry1).unwrap();
    registry.register(entry2).unwrap();
    registry.register(entry3).unwrap();

    assert_eq!(registry.len(), 3);

    let maya_instances = registry.list_instances("maya");
    assert_eq!(maya_instances.len(), 2);

    let blender_instances = registry.list_instances("blender");
    assert_eq!(blender_instances.len(), 1);
}

#[test]
fn test_file_registry_recreates_registry_dir_before_write() {
    let dir = tempfile::tempdir().unwrap();
    let registry_dir = dir.path().to_path_buf();
    let registry = FileRegistry::new(&registry_dir).unwrap();
    drop(dir);

    assert!(!registry_dir.exists());

    registry
        .register(ServiceEntry::new("maya", "127.0.0.1", 18812))
        .unwrap();

    assert!(registry.registry_lock_path().exists());
    assert!(registry.registry_file_path().exists());
    assert_eq!(FileRegistry::new(&registry_dir).unwrap().len(), 1);
}

#[test]
fn test_file_registry_deregister() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();
    assert_eq!(registry.len(), 1);

    let removed = registry.deregister(&key).unwrap();
    assert!(removed.is_some());
    assert!(registry.is_empty());
}

#[test]
fn test_file_registry_persistence() {
    let dir = tempfile::tempdir().unwrap();

    let instance_id;
    {
        let registry = FileRegistry::new(dir.path()).unwrap();
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        instance_id = entry.instance_id;
        registry.register(entry).unwrap();
    }

    // Reload from file
    let registry = FileRegistry::new(dir.path()).unwrap();
    assert_eq!(registry.len(), 1);
    let entries = registry.list_instances("maya");
    assert_eq!(entries[0].instance_id, instance_id);
}

#[test]
fn test_file_registry_heartbeat() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();

    assert!(registry.heartbeat(&key).unwrap());

    // Non-existent key
    let fake_key = ServiceKey {
        dcc_type: "nuke".to_string(),
        instance_id: Uuid::new_v4(),
    };
    assert!(!registry.heartbeat(&fake_key).unwrap());
}

#[test]
fn test_file_registry_cleanup_stale() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    // Force old heartbeat
    entry.last_heartbeat = std::time::SystemTime::now() - std::time::Duration::from_secs(100);
    registry.register(entry).unwrap();

    let cleaned = registry
        .cleanup_stale(std::time::Duration::from_secs(10))
        .unwrap();
    assert_eq!(cleaned, 1);
    assert!(registry.is_empty());
}

// Regression test for issue #230: cleanup_stale must never evict the gateway sentinel,
// even when its heartbeat appears stale, because that record is the source of truth
// for "who is the gateway" and a live but non-heartbeating sentinel is valid.
#[test]
fn test_file_registry_cleanup_stale_preserves_gateway_sentinel() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let mut sentinel = ServiceEntry::new(GATEWAY_SENTINEL_DCC_TYPE, "127.0.0.1", 9765);
    sentinel.last_heartbeat = std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    registry.register(sentinel).unwrap();

    let mut stale_instance = ServiceEntry::new("maya", "127.0.0.1", 18812);
    stale_instance.last_heartbeat =
        std::time::SystemTime::now() - std::time::Duration::from_secs(600);
    registry.register(stale_instance).unwrap();

    let cleaned = registry
        .cleanup_stale(std::time::Duration::from_secs(30))
        .unwrap();
    // Only the maya row gets evicted; sentinel survives.
    assert_eq!(cleaned, 1);
    assert_eq!(registry.len(), 1);
    assert_eq!(
        registry.list_instances(GATEWAY_SENTINEL_DCC_TYPE).len(),
        1,
        "gateway sentinel must not be evicted by cleanup_stale"
    );
}

// Regression test for issue #227: ghost rows from a crashed DCC process must be reaped.
#[test]
fn test_file_registry_prune_dead_pids() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Live entry (auto-populated pid == our own process id).
    let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let live_key = live.key();
    registry.register(live).unwrap();

    // Ghost entry with a clearly-dead PID.
    // u32::MAX is a reserved sentinel on every OS we target.
    let ghost = ServiceEntry::new("maya", "127.0.0.1", 18813).with_pid(u32::MAX);
    let ghost_key = ghost.key();
    registry.register(ghost).unwrap();
    remove_sentinel_for_pid_fallback(&registry, &ghost_key);

    let pruned = registry.prune_dead_pids().unwrap();
    assert_eq!(pruned, 1, "exactly one ghost entry should be pruned");
    assert!(registry.get(&live_key).is_some(), "live entry must remain");
    assert!(
        registry.get(&ghost_key).is_none(),
        "ghost entry must be removed"
    );
}

#[test]
fn test_file_registry_sentinel_survives_dead_pid_while_lock_held() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812).with_pid(u32::MAX);
    let key = entry.key();
    registry.register(entry).unwrap();

    assert_eq!(registry.prune_dead_entries().unwrap(), 0);
    assert!(registry.get(&key).is_some());
}

#[test]
fn test_file_registry_sentinel_prunes_after_owner_drops_lock() {
    let dir = tempfile::tempdir().unwrap();
    let key = {
        let registry = FileRegistry::new(dir.path()).unwrap();
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        registry.register(entry).unwrap();
        key
    };

    let reader = FileRegistry::new(dir.path()).unwrap();
    let pruned = reader.prune_dead_entries().unwrap();

    assert_eq!(pruned, 1);
    assert!(reader.get(&key).is_none());
}

#[test]
fn test_file_registry_prune_dead_pids_skips_unknown_pid() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Entry with pid explicitly cleared → liveness unknown, must not be pruned.
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    entry.pid = None;
    registry.register(entry).unwrap();

    let pruned = registry.prune_dead_pids().unwrap();
    assert_eq!(pruned, 0);
    assert_eq!(registry.len(), 1);
}

/// Regression for issue #719: a reader `FileRegistry` that never saw
/// another process's writes must still evict that process's ghost
/// rows when it next calls `prune_dead_entries`. Before the
/// `reload_if_stale()` guard at the top of `prune_dead_entries` the
/// reader's in-memory cache stayed empty, so there was nothing to
/// iterate and the ghost survived every sweep.
#[test]
fn test_prune_dead_entries_reloads_before_iterating() {
    let dir = tempfile::tempdir().unwrap();

    // Writer registers a ghost row, then drops — releasing the
    // sentinel lock as a real crashed DCC process would.
    let ghost_key = {
        let writer = FileRegistry::new(dir.path()).unwrap();
        let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
        let key = entry.key();
        writer.register(entry).unwrap();
        key
    };

    // Reader is created AFTER the writer dropped — so `new()` does
    // load_from_file and sees the row, but that's the one code path
    // that already reloads. To prove `prune_dead_entries` itself now
    // reloads, we need a reader that was already alive when the
    // ghost was written.
    let dir2 = tempfile::tempdir().unwrap();
    let early_reader = FileRegistry::new(dir2.path()).unwrap();
    assert!(early_reader.is_empty());

    // Writer appears in the same dir as `early_reader`.
    let ghost_key2 = {
        let writer = FileRegistry::new(dir2.path()).unwrap();
        let entry = ServiceEntry::new("blender", "127.0.0.1", 18813);
        let key = entry.key();
        writer.register(entry).unwrap();
        key
    };

    // Before reload `early_reader` has nothing in memory — a pre-#719
    // prune would iterate an empty map and return 0. The current
    // impl must reload, see the row, notice the sentinel lock is
    // released, and evict it.
    let pruned = early_reader.prune_dead_entries().unwrap();
    assert_eq!(pruned, 1, "early reader must evict the cross-process ghost");
    assert!(early_reader.get(&ghost_key2).is_none());

    // Keep the first-dir ghost_key referenced so the block above is
    // not optimised out under strict lint profiles.
    assert!(!ghost_key.instance_id.is_nil());
}

#[test]
fn test_prune_dead_entries_ignores_cached_mtime_when_pruning() {
    let dir = tempfile::tempdir().unwrap();
    let reader = FileRegistry::new(dir.path()).unwrap();

    let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
    reader.register(live).unwrap();

    {
        let writer = FileRegistry::new(dir.path()).unwrap();
        let ghost = ServiceEntry::new("blender", "127.0.0.1", 18813);
        writer.register(ghost).unwrap();
    }

    let registry_path = reader.registry_file_path();
    let current_mtime = std::fs::metadata(registry_path)
        .unwrap()
        .modified()
        .unwrap();
    *reader.last_mtime.lock().unwrap() = Some(current_mtime);

    let pruned = reader.prune_dead_entries().unwrap();
    assert_eq!(
        pruned, 1,
        "prune must reload even when cached mtime matches the registry file"
    );
}

#[test]
fn test_file_registry_multiple_instances_same_dcc() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Register multiple Maya instances — this is the critical fix
    for port in 18812..18815 {
        let entry = ServiceEntry::new("maya", "127.0.0.1", port);
        registry.register(entry).unwrap();
    }

    assert_eq!(registry.len(), 3);
    let maya_instances = registry.list_instances("maya");
    assert_eq!(maya_instances.len(), 3);

    // Each should have a unique port
    let ports: Vec<u16> = maya_instances.iter().map(|e| e.port).collect();
    assert!(ports.contains(&18812));
    assert!(ports.contains(&18813));
    assert!(ports.contains(&18814));
}

#[test]
fn test_file_registry_acquire_and_release_lease() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();

    let leased = registry
        .acquire_lease(
            "maya",
            None,
            "workflow-1",
            Some("job-1".to_string()),
            Some(Duration::from_secs(60)),
        )
        .unwrap()
        .expect("idle instance should be leased");
    assert_eq!(leased.status, ServiceStatus::Busy);
    assert_eq!(leased.lease_owner.as_deref(), Some("workflow-1"));
    assert_eq!(leased.current_job_id.as_deref(), Some("job-1"));

    assert!(
        registry
            .acquire_lease("maya", None, "workflow-2", None, None)
            .unwrap()
            .is_none(),
        "busy leased instance must not be acquired twice"
    );

    let released = registry
        .release_lease(&key, Some("workflow-1"))
        .unwrap()
        .expect("matching owner can release");
    assert_eq!(released.status, ServiceStatus::Available);
    assert!(released.lease_owner.is_none());
}

#[test]
fn test_file_registry_acquire_clears_expired_lease() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    entry.acquire_lease(
        "old-owner",
        Some("old-job".to_string()),
        Some(SystemTime::now() - Duration::from_secs(1)),
    );
    registry.register(entry).unwrap();

    let leased = registry
        .acquire_lease(
            "maya",
            None,
            "new-owner",
            None,
            Some(Duration::from_secs(60)),
        )
        .unwrap()
        .expect("expired lease should be reusable");

    assert_eq!(leased.status, ServiceStatus::Busy);
    assert_eq!(leased.lease_owner.as_deref(), Some("new-owner"));
    assert!(leased.current_job_id.is_none());
}

#[test]
fn test_file_registry_hot_reload() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Register entry in process A
    let entry_a = ServiceEntry::new("maya", "127.0.0.1", 18812);
    registry.register(entry_a).unwrap();
    assert_eq!(registry.len(), 1);

    // Small sleep to ensure filesystem mtime granularity is observed
    // (on some systems, mtime has 1-second or coarser precision)
    std::thread::sleep(Duration::from_millis(100));

    // Simulate external write by another process: create a new registry instance
    // that writes a new entry to the same file
    {
        let registry_b = FileRegistry::new(dir.path()).unwrap();
        let entry_b = ServiceEntry::new("blender", "127.0.0.1", 8888);
        registry_b.register(entry_b).unwrap();
    }

    // Process A should detect the new entry via hot-reload
    let all = registry.list_all();
    assert_eq!(all.len(), 2, "hot-reload should discover external entry");

    let maya = registry.list_instances("maya");
    assert_eq!(maya.len(), 1);

    let blender = registry.list_instances("blender");
    assert_eq!(blender.len(), 1);
}

#[test]
fn test_file_registry_hot_reload_is_lazy() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Register initial entry
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    registry.register(entry).unwrap();

    // Multiple list_all() calls on unchanged file should all hit fast path
    for _ in 0..5 {
        let _ = registry.list_all();
    }

    // All calls should succeed without error
    assert_eq!(registry.len(), 1);
}

#[test]
fn test_heartbeat_survives_external_write_between_register_and_flush() {
    let dir = tempfile::tempdir().unwrap();
    let registry_a = FileRegistry::new(dir.path()).unwrap();

    let sidecar = ServiceEntry::new("3dsmax", "127.0.0.1", 57677);
    let sidecar_key = sidecar.key();
    registry_a.register(sidecar).unwrap();
    let registered_heartbeat = registry_a.get(&sidecar_key).unwrap().last_heartbeat;

    std::thread::sleep(Duration::from_millis(100));

    {
        let registry_b = FileRegistry::new(dir.path()).unwrap();
        let maya = ServiceEntry::new("maya", "127.0.0.1", 50492);
        registry_b.register(maya).unwrap();
    }

    std::thread::sleep(Duration::from_millis(100));
    assert!(registry_a.heartbeat(&sidecar_key).unwrap());

    let reread = FileRegistry::new(dir.path()).unwrap();
    let sidecar_after = reread
        .get(&sidecar_key)
        .expect("sidecar row must survive heartbeat");
    assert!(
        sidecar_after.last_heartbeat > registered_heartbeat,
        "heartbeat must persist the touched timestamp after another registry handle wrote services.json"
    );
    assert_eq!(
        reread.list_instances("maya").len(),
        1,
        "heartbeat flush must preserve rows written by other registry handles"
    );
}

#[test]
fn test_heartbeat_does_not_resurrect_externally_deregistered_row() {
    let dir = tempfile::tempdir().unwrap();
    let registry_a = FileRegistry::new(dir.path()).unwrap();

    let sidecar = ServiceEntry::new("3dsmax", "127.0.0.1", 57677);
    let sidecar_key = sidecar.key();
    registry_a.register(sidecar).unwrap();

    let maya_key = {
        let registry_b = FileRegistry::new(dir.path()).unwrap();
        let maya = ServiceEntry::new("maya", "127.0.0.1", 50492);
        let maya_key = maya.key();
        registry_b.register(maya).unwrap();
        registry_a.list_all();
        registry_b.deregister(&maya_key).unwrap();
        maya_key
    };

    assert!(
        registry_a.heartbeat(&sidecar_key).unwrap(),
        "sidecar heartbeat should still succeed"
    );

    let reread = FileRegistry::new(dir.path()).unwrap();
    assert!(
        reread.get(&maya_key).is_none(),
        "heartbeat flush must not resurrect a row removed by another registry handle"
    );
}

#[test]
fn test_write_transaction_times_out_when_in_process_lock_is_held() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new_with_lock_policy(
        dir.path(),
        Duration::from_millis(40),
        Duration::from_millis(5),
    )
    .unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();

    let _held = registry.write_lock.lock().unwrap();
    let started = std::time::Instant::now();
    let err = registry.heartbeat(&key).unwrap_err();

    assert!(
        started.elapsed() < Duration::from_secs(1),
        "bounded write lock wait must not hang the caller"
    );
    assert!(
        err.to_string()
            .contains("timed out waiting for in-process registry mutex"),
        "unexpected error: {err}"
    );
}

#[test]
fn test_heartbeat_merges_legacy_unlocked_writer_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let sidecar = ServiceEntry::new("3dsmax", "127.0.0.1", 57677);
    let sidecar_key = sidecar.key();
    registry.register(sidecar).unwrap();
    let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", 18080);
    let photoshop_key = photoshop.key();
    registry.register(photoshop).unwrap();
    let before = registry.get(&sidecar_key).unwrap().last_heartbeat;
    std::thread::sleep(Duration::from_millis(20));

    let legacy = ServiceEntry::new("maya", "127.0.0.1", 50492);
    let legacy_key = legacy.key();
    let legacy_path = dir.path().join("services.json");
    set_before_transaction_flush_hook(dir.path(), move || {
        let content = serde_json::to_string_pretty(&vec![legacy]).unwrap();
        std::fs::write(&legacy_path, content).unwrap();
    });

    assert!(
        registry.heartbeat(&sidecar_key).unwrap(),
        "sidecar heartbeat should still succeed"
    );

    let reread = FileRegistry::new(dir.path()).unwrap();
    let sidecar_after = reread
        .get(&sidecar_key)
        .expect("new writer row must survive legacy unlocked overwrite");
    assert!(
        sidecar_after.last_heartbeat > before,
        "new writer's heartbeat update must win for its own row"
    );
    assert!(
        reread.get(&legacy_key).is_some(),
        "legacy unlocked writer row must be merged instead of being lost"
    );
    assert!(
        reread.get(&photoshop_key).is_some(),
        "unchanged baseline rows must survive a legacy stale snapshot"
    );
}

#[test]
fn test_missing_registry_file_during_transaction_clears_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let sidecar = ServiceEntry::new("3dsmax", "127.0.0.1", 57677);
    let sidecar_key = sidecar.key();
    registry.register(sidecar).unwrap();
    std::fs::remove_file(registry.registry_file_path()).unwrap();

    assert!(
        !registry.heartbeat(&sidecar_key).unwrap(),
        "deleted services.json is authoritative during a transaction"
    );
    assert!(
        FileRegistry::new(dir.path()).unwrap().is_empty(),
        "missing services.json must not be repopulated from stale memory"
    );
}

#[test]
fn test_empty_registry_file_during_transaction_clears_snapshot() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let sidecar = ServiceEntry::new("3dsmax", "127.0.0.1", 57677);
    let sidecar_key = sidecar.key();
    registry.register(sidecar).unwrap();
    std::fs::write(registry.registry_file_path(), "").unwrap();

    assert!(
        !registry.heartbeat(&sidecar_key).unwrap(),
        "empty services.json is authoritative during a transaction"
    );
    assert!(
        FileRegistry::new(dir.path()).unwrap().is_empty(),
        "empty services.json must not be repopulated from stale memory"
    );
}

#[test]
fn test_zero_padded_registry_file_is_quarantined_and_starts_empty() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(REGISTRY_FILE);
    std::fs::write(&path, vec![0u8; 1466]).unwrap();

    let registry = FileRegistry::new(dir.path()).unwrap();

    assert!(registry.is_empty());
    assert!(!path.exists());
    let quarantined = corrupted_registry_files(dir.path());
    assert_eq!(quarantined.len(), 1);
    assert_eq!(std::fs::metadata(&quarantined[0]).unwrap().len(), 1466);
    assert!(
        std::fs::read(&quarantined[0])
            .unwrap()
            .iter()
            .all(|byte| *byte == 0)
    );
}

#[test]
fn test_zero_padded_registry_file_with_trailing_newline_is_quarantined() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(REGISTRY_FILE);
    std::fs::write(&path, [0, 0, 0, b'\r', b'\n']).unwrap();

    let registry = FileRegistry::new(dir.path()).unwrap();

    assert!(registry.is_empty());
    assert!(!path.exists());
    let quarantined = corrupted_registry_files(dir.path());
    assert_eq!(quarantined.len(), 1);
    assert_eq!(
        std::fs::read(&quarantined[0]).unwrap(),
        vec![0, 0, 0, b'\r', b'\n']
    );
}

#[test]
fn test_whitespace_only_registry_file_stays_empty_without_quarantine() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join(REGISTRY_FILE);
    std::fs::write(&path, " \r\n\t").unwrap();

    let registry = FileRegistry::new(dir.path()).unwrap();

    assert!(registry.is_empty());
    assert!(path.exists());
    assert!(corrupted_registry_files(dir.path()).is_empty());
}

#[test]
fn test_file_registry_update_metadata() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();

    // Initially no scene
    let e = registry.get(&key).unwrap();
    assert!(e.scene.is_none());
    assert!(e.version.is_none());

    // Update scene
    assert!(
        registry
            .update_metadata(&key, Some("my_scene.ma"), None)
            .unwrap()
    );
    let e = registry.get(&key).unwrap();
    assert_eq!(e.scene.as_deref(), Some("my_scene.ma"));
    assert!(e.version.is_none());

    // Update version
    assert!(registry.update_metadata(&key, None, Some("2025")).unwrap());
    let e = registry.get(&key).unwrap();
    assert_eq!(e.scene.as_deref(), Some("my_scene.ma"));
    assert_eq!(e.version.as_deref(), Some("2025"));

    // Update both
    assert!(
        registry
            .update_metadata(&key, Some("other.ma"), Some("2026"))
            .unwrap()
    );
    let e = registry.get(&key).unwrap();
    assert_eq!(e.scene.as_deref(), Some("other.ma"));
    assert_eq!(e.version.as_deref(), Some("2026"));

    // Clear scene with empty string
    assert!(registry.update_metadata(&key, Some(""), None).unwrap());
    let e = registry.get(&key).unwrap();
    assert!(e.scene.is_none());

    // Non-existent key
    let fake_key = ServiceKey {
        dcc_type: "nuke".to_string(),
        instance_id: Uuid::new_v4(),
    };
    assert!(
        !registry
            .update_metadata(&fake_key, Some("x"), None)
            .unwrap()
    );
}

// ── read_alive auto-eviction (issue #523) ─────────────────────────────────

#[test]
fn test_read_alive_evicts_ghost_and_returns_live() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    // Live entry (auto-populated pid == our own process id).
    let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let live_key = live.key();
    registry.register(live).unwrap();

    // Ghost entry with a clearly-dead PID.
    let ghost = ServiceEntry::new("maya", "127.0.0.1", 18813).with_pid(u32::MAX);
    let ghost_key = ghost.key();
    registry.register(ghost).unwrap();
    remove_sentinel_for_pid_fallback(&registry, &ghost_key);

    let (entries, evicted) = registry.read_alive().unwrap();
    assert_eq!(evicted, 1, "exactly one ghost row must be evicted");
    assert_eq!(entries.len(), 1, "only the live row must remain");
    assert!(registry.get(&live_key).is_some());
    assert!(registry.get(&ghost_key).is_none());
}

#[test]
fn test_read_alive_returns_zero_evicted_when_all_live() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let entry_a = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let entry_b = ServiceEntry::new("blender", "127.0.0.1", 9090);
    registry.register(entry_a).unwrap();
    registry.register(entry_b).unwrap();

    let (entries, evicted) = registry.read_alive().unwrap();
    assert_eq!(evicted, 0);
    assert_eq!(entries.len(), 2);
}

#[test]
fn test_read_alive_idempotent_after_first_call() {
    // Once a ghost has been evicted, subsequent read_alive calls report
    // evicted == 0 and the file rewrite is not repeated.
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
    registry.register(live).unwrap();
    let ghost = ServiceEntry::new("maya", "127.0.0.1", 18813).with_pid(u32::MAX);
    let ghost_key = ghost.key();
    registry.register(ghost).unwrap();
    remove_sentinel_for_pid_fallback(&registry, &ghost_key);

    let (_, evicted_first) = registry.read_alive().unwrap();
    assert_eq!(evicted_first, 1);
    let (entries, evicted_second) = registry.read_alive().unwrap();
    assert_eq!(evicted_second, 0);
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_read_alive_with_log_returns_same_result() {
    // The logging variant must behave identically apart from emitting a
    // warn on the threshold cross — verify the data path here, leave the
    // log assertion to a manual smoke (tracing-test would be a heavy dep).
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let live = ServiceEntry::new("maya", "127.0.0.1", 18812);
    registry.register(live).unwrap();
    let ghost = ServiceEntry::new("maya", "127.0.0.1", 18813).with_pid(u32::MAX);
    let ghost_key = ghost.key();
    registry.register(ghost).unwrap();
    remove_sentinel_for_pid_fallback(&registry, &ghost_key);

    // Threshold larger than the eviction count → no warn, but counts match.
    let (entries, evicted) = registry.read_alive_with_log(100).unwrap();
    assert_eq!(evicted, 1);
    assert_eq!(entries.len(), 1);
}

#[test]
fn test_read_alive_handles_multiple_ghosts_for_dcc_maya_126() {
    // Reproduces loonghao/dcc-mcp-maya#126: gateway sees N stale instances
    // because Maya crashed N times without cleanup. read_alive should
    // evict them all in a single sweep.
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    for port in 18812..18812 + 5 {
        let ghost = ServiceEntry::new("maya", "127.0.0.1", port).with_pid(u32::MAX - port as u32);
        let ghost_key = ghost.key();
        registry.register(ghost).unwrap();
        remove_sentinel_for_pid_fallback(&registry, &ghost_key);
    }
    let live = ServiceEntry::new("maya", "127.0.0.1", 19000);
    registry.register(live).unwrap();

    let (entries, evicted) = registry.read_alive().unwrap();
    assert_eq!(evicted, 5);
    assert_eq!(entries.len(), 1);
}

/// Regression for the #560 follow-up: two threads heartbeating different
/// `ServiceEntry` rows at the same time must both succeed and the resulting
/// `services.json` must contain *both* entries, not just one.
///
/// One broken #554-era implementation opened a shared lock file with
/// `share_mode(0)` (exclusive) — concurrent writers would fail the lock open
/// with `PermissionDenied` and silently drop their entry, which manifested as
/// the gateway facade only seeing one of two backends in
/// `tests/test_gateway_facade_aggregation.py` on Windows. The current path
/// uses a shared `services.lock` with bounded try-lock/backoff.
#[test]
fn test_concurrent_heartbeat_does_not_drop_entries() {
    use std::sync::Arc;
    use std::thread;

    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(FileRegistry::new(dir.path()).unwrap());

    // Pre-register both rows so each thread only heartbeats (the path that
    // hammered `flush_to_file` in production).
    let maya = ServiceEntry::new("maya", "127.0.0.1", 18900);
    let blender = ServiceEntry::new("blender", "127.0.0.1", 18901);
    let maya_key = maya.key();
    let blender_key = blender.key();
    registry.register(maya).unwrap();
    registry.register(blender).unwrap();

    let r1 = Arc::clone(&registry);
    let r2 = Arc::clone(&registry);
    let t1 = thread::spawn(move || {
        for _ in 0..50 {
            r1.heartbeat(&maya_key).unwrap();
        }
    });
    let t2 = thread::spawn(move || {
        for _ in 0..50 {
            r2.heartbeat(&blender_key).unwrap();
        }
    });
    t1.join().unwrap();
    t2.join().unwrap();

    // Re-load from disk in a second registry to verify both entries
    // survived every flush, not just the in-memory snapshot.
    let reread = FileRegistry::new(dir.path()).unwrap();
    let dccs: std::collections::HashSet<_> =
        reread.list_all().into_iter().map(|e| e.dcc_type).collect();
    assert!(
        dccs.contains("maya"),
        "maya entry lost in concurrent heartbeat"
    );
    assert!(
        dccs.contains("blender"),
        "blender entry lost in concurrent heartbeat"
    );
}

/// Regression test for issue #853 — `write_atomic` must produce exactly one
/// stable temp filename (`.tmp.<pid>.services.json`) across multiple
/// concurrent flushes within the same process, rather than a fresh
/// `(pid, tid, seq)` path per write.
///
/// The test verifies the invariant by:
/// 1. Running N concurrent heartbeat threads, each flushing several times.
/// 2. Scanning the registry directory for `.tmp.*` files after the flushes.
/// 3. Asserting that no file whose name contains a thread-id or sequence
///    number fragment ever appears on disk.
#[test]
fn test_write_atomic_stable_temp_filename() {
    let dir = tempfile::tempdir().unwrap();
    let registry = std::sync::Arc::new(
        FileRegistry::new_with_lock_policy(
            dir.path(),
            Duration::from_secs(10),
            Duration::from_millis(5),
        )
        .unwrap(),
    );

    // Register an entry so there is something to flush.
    let mut entry = ServiceEntry::new("maya", "127.0.0.1", 7001);
    entry.instance_id = Uuid::new_v4();
    let key = entry.key();
    registry.register(entry).unwrap();

    // Spawn N threads that flush concurrently.
    const THREADS: usize = 4;
    const FLUSHES_PER_THREAD: usize = 8;
    let mut handles = Vec::with_capacity(THREADS);
    for _ in 0..THREADS {
        let r = std::sync::Arc::clone(&registry);
        let k = key.clone();
        handles.push(std::thread::spawn(move || {
            for _ in 0..FLUSHES_PER_THREAD {
                r.heartbeat(&k).unwrap();
            }
        }));
    }
    for h in handles {
        h.join().unwrap();
    }

    // After all flushes, the registry dir must contain at most one
    // `.tmp.*` file (the stable one), and if it exists its name must
    // match exactly `.tmp.<pid>.services.json`.
    let pid = std::process::id();
    let stable_name = format!(".tmp.{pid}.services.json");
    let tmp_files: Vec<_> = std::fs::read_dir(dir.path())
        .unwrap()
        .filter_map(|e| e.ok())
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tmp."))
        .collect();

    for name in &tmp_files {
        assert_eq!(
            name, &stable_name,
            "unexpected temp filename found — expected only '{stable_name}', got '{name}'"
        );
    }
    // The stable file is usually renamed away; it is acceptable (but not
    // required) for it to be absent after all flushes complete.
}

#[test]
fn test_write_atomic_sync_failure_does_not_replace_registry_and_cleans_temp() {
    let dir = tempfile::tempdir().unwrap();
    let registry = FileRegistry::new(dir.path()).unwrap();

    let maya = ServiceEntry::new("maya", "127.0.0.1", 7001);
    let maya_key = maya.key();
    registry.register(maya).unwrap();

    let registry_path = dir.path().join(REGISTRY_FILE);
    let baseline = std::fs::read_to_string(&registry_path).unwrap();
    assert!(baseline.contains("maya"));

    set_before_temp_sync_hook(dir.path(), |temp_path| {
        let temp_content = std::fs::read_to_string(temp_path).unwrap();
        assert!(
            temp_content.contains("photoshop"),
            "temp file should contain the new snapshot before fsync"
        );
        Err(std::io::Error::other("simulated crash before fsync"))
    });

    let photoshop = ServiceEntry::new("photoshop", "127.0.0.1", 7002);
    let photoshop_key = photoshop.key();
    let err = registry.register(photoshop).unwrap_err();
    let err = err.to_string();
    assert!(err.contains("failed to sync temp file"), "{err}");
    assert!(err.contains("simulated crash before fsync"), "{err}");

    assert_eq!(
        std::fs::read_to_string(&registry_path).unwrap(),
        baseline,
        "a failed temp-file fsync must not replace the last durable services.json"
    );
    assert!(registry.get(&maya_key).is_some());
    assert!(
        registry.get(&photoshop_key).is_none(),
        "a failed write must roll back the current registry handle"
    );
    assert!(
        registry
            .list_all()
            .into_iter()
            .all(|entry| entry.key() != photoshop_key),
        "the failed write must not leak into list_all on the current handle"
    );
    assert!(
        !registry.heartbeat(&photoshop_key).unwrap(),
        "heartbeat must not re-persist an entry from the failed transaction"
    );
    assert!(
        !registry.sentinel_handles.contains_key(&photoshop_key),
        "failed register must not retain ownership of the new sentinel"
    );
    assert!(
        temp_registry_file_names(dir.path()).is_empty(),
        "failed fsync should clean up the stable temp file"
    );

    let reread = FileRegistry::new(dir.path()).unwrap();
    assert!(reread.get(&maya_key).is_some());
    assert!(
        reread.get(&photoshop_key).is_none(),
        "the failed write must not leak into the durable registry"
    );
}

#[test]
fn classify_lock_wait_stays_quiet_for_brief_contention() {
    let backoff = Duration::from_millis(10);
    let slow = Duration::from_millis(250);

    // A wait equal to a single backoff tick (the common "acquired after one
    // retry" case, e.g. waited_ms=10) must not produce any log line — this is
    // the noisy-warning regression we are guarding against.
    assert_eq!(
        classify_lock_wait(Duration::from_millis(10), backoff, slow),
        LockWaitLevel::Quiet
    );
    assert_eq!(
        classify_lock_wait(Duration::ZERO, backoff, slow),
        LockWaitLevel::Quiet
    );
    // Just under two backoff ticks is still quiet.
    assert_eq!(
        classify_lock_wait(Duration::from_millis(19), backoff, slow),
        LockWaitLevel::Quiet
    );
}

#[test]
fn classify_lock_wait_debugs_only_after_multiple_backoffs() {
    let backoff = Duration::from_millis(10);
    let slow = Duration::from_millis(250);

    // Two or more backoff ticks but below the slow threshold → debug-level retry.
    assert_eq!(
        classify_lock_wait(Duration::from_millis(20), backoff, slow),
        LockWaitLevel::Retry
    );
    assert_eq!(
        classify_lock_wait(Duration::from_millis(120), backoff, slow),
        LockWaitLevel::Retry
    );
}

#[test]
fn classify_lock_wait_warns_only_when_genuinely_slow() {
    let backoff = Duration::from_millis(10);
    let slow = Duration::from_millis(250);

    assert_eq!(
        classify_lock_wait(Duration::from_millis(250), backoff, slow),
        LockWaitLevel::Slow
    );
    assert_eq!(
        classify_lock_wait(Duration::from_millis(1_999), backoff, slow),
        LockWaitLevel::Slow
    );
}
