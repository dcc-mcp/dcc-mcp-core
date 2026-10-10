use super::*;

#[test]
fn test_all_cross_protocol_traits_available() {
    let adapter = MockDccAdapter::new();
    assert!(adapter.as_scene_manager().is_some());
    assert!(adapter.as_transform().is_some());
    assert!(adapter.as_render_capture().is_some());
    assert!(adapter.as_hierarchy().is_some());
}

#[test]
fn test_render_capture_none_when_snapshot_disabled() {
    let config = MockConfig::builder().snapshot_enabled(false).build();
    let adapter = MockDccAdapter::with_config(config);
    assert!(adapter.as_render_capture().is_none());
    // Other traits still available
    assert!(adapter.as_scene_manager().is_some());
    assert!(adapter.as_transform().is_some());
    assert!(adapter.as_hierarchy().is_some());
}

#[test]
fn test_photoshop_preset_has_all_traits() {
    let config = MockConfig::photoshop("25.0");
    let adapter = MockDccAdapter::with_config(config);
    assert!(adapter.as_scene_manager().is_some());
    assert!(adapter.as_transform().is_some());
    assert!(adapter.as_render_capture().is_some());
    assert!(adapter.as_hierarchy().is_some());
    assert_eq!(adapter.info().dcc_type, "photoshop");
}

#[test]
fn test_unity_preset_has_all_traits() {
    let config = MockConfig::unity("2022.3");
    let adapter = MockDccAdapter::with_config(config);
    assert!(adapter.as_scene_manager().is_some());
    assert!(adapter.as_hierarchy().is_some());
    assert!(adapter.info().python_version.is_none());
}
