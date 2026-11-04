//! Concurrent readers must see complete registry snapshots during publication.

use super::*;
use std::sync::{Arc, Barrier};

fn registry_with_entries(entries: &[ServiceEntry]) -> (tempfile::TempDir, Arc<FileRegistry>) {
    let dir = tempfile::tempdir().unwrap();
    std::fs::write(
        dir.path().join(REGISTRY_FILE),
        serde_json::to_vec(entries).unwrap(),
    )
    .unwrap();
    let registry = Arc::new(FileRegistry::new(dir.path()).unwrap());
    (dir, registry)
}

fn assert_complete_snapshot(entries: &[ServiceEntry], expected_keys: &HashSet<ServiceKey>) {
    let keys = entries
        .iter()
        .map(ServiceEntry::key)
        .collect::<HashSet<_>>();
    assert_eq!(
        &keys, expected_keys,
        "reader observed incomplete membership"
    );
    let generation = entries[0].port;
    assert!(
        entries.iter().all(|entry| entry.port == generation),
        "reader observed mixed snapshot generations"
    );
}

#[test]
fn concurrent_snapshot_publication_preserves_complete_membership() {
    let first = (0..64)
        .map(|_| ServiceEntry::new("maya", "127.0.0.1", 18812))
        .collect::<Vec<_>>();
    let second = first
        .iter()
        .cloned()
        .map(|mut entry| {
            entry.port = 18813;
            entry
        })
        .collect::<Vec<_>>();
    let keys = first.iter().map(ServiceEntry::key).collect::<HashSet<_>>();
    let key = first[0].key();
    let (_dir, registry) = registry_with_entries(&first);
    let start = Barrier::new(3);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for iteration in 0..2_000 {
                registry.replace_services(if iteration % 2 == 0 { &second } else { &first });
            }
        });
        for _ in 0..2 {
            scope.spawn(|| {
                start.wait();
                for _ in 0..1_000 {
                    assert_complete_snapshot(&registry.list_all(), &keys);
                    assert_complete_snapshot(&registry.list_instances("maya"), &keys);
                    assert!(registry.get(&key).is_some(), "stable instance disappeared");
                    assert_eq!(registry.len(), keys.len());
                    assert!(!registry.is_empty());
                }
            });
        }
    });
}

#[test]
fn heartbeat_keeps_the_registered_instance_visible_to_concurrent_readers() {
    let dir = tempfile::tempdir().unwrap();
    let registry = Arc::new(FileRegistry::new(dir.path()).unwrap());
    let entry = ServiceEntry::new("maya", "127.0.0.1", 18812);
    let key = entry.key();
    registry.register(entry).unwrap();
    let start = Barrier::new(2);

    std::thread::scope(|scope| {
        scope.spawn(|| {
            start.wait();
            for _ in 0..64 {
                assert!(registry.heartbeat(&key).unwrap());
            }
        });
        start.wait();
        for _ in 0..4_000 {
            assert_eq!(registry.list_all().len(), 1);
            assert_eq!(registry.list_instances("maya").len(), 1);
            assert!(registry.get(&key).is_some());
            assert_eq!(registry.len(), 1);
            assert!(!registry.is_empty());
        }
    });

    let reloaded = FileRegistry::new(dir.path()).unwrap();
    assert_eq!(reloaded.list_all().len(), 1);
    assert!(reloaded.get(&key).is_some());
}
