//! Unit tests for `FileRegistry`.

use super::*;
use uuid::Uuid;

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

    let pruned = registry.prune_dead_pids().unwrap();
    assert_eq!(pruned, 1, "exactly one ghost entry should be pruned");
    assert!(registry.get(&live_key).is_some(), "live entry must remain");
    assert!(
        registry.get(&ghost_key).is_none(),
        "ghost entry must be removed"
    );
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
    registry.register(ghost).unwrap();

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
    registry.register(ghost).unwrap();

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
        registry.register(ghost).unwrap();
    }
    let live = ServiceEntry::new("maya", "127.0.0.1", 19000);
    registry.register(live).unwrap();

    let (entries, evicted) = registry.read_alive().unwrap();
    assert_eq!(evicted, 5);
    assert_eq!(entries.len(), 1);
}
