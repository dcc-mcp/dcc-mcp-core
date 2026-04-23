use super::*;

#[test]
fn test_capabilities() {
    let adapter = MockDccAdapter::new();
    let caps = adapter.capabilities();
    assert!(caps.scene_info);
    assert!(caps.snapshot);
    assert!(caps.selection);
    assert!(caps.file_operations);
}

#[test]
fn test_as_connection() {
    let mut adapter = MockDccAdapter::new();
    assert!(adapter.as_connection().is_some());
}

#[test]
fn test_as_script_engine() {
    let adapter = MockDccAdapter::new();
    assert!(adapter.as_script_engine().is_some());
}

#[test]
fn test_as_scene_info() {
    let adapter = MockDccAdapter::new();
    assert!(adapter.as_scene_info().is_some());
}

#[test]
fn test_as_snapshot_enabled() {
    let adapter = MockDccAdapter::new();
    assert!(adapter.as_snapshot().is_some());
}

#[test]
fn test_as_snapshot_disabled() {
    let config = MockConfig::builder().snapshot_enabled(false).build();
    let adapter = MockDccAdapter::with_config(config);
    assert!(adapter.as_snapshot().is_none());
}
