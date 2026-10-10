use super::*;

#[test]
fn test_invocation_counters() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();
    adapter.connect().unwrap(); // connect twice

    adapter
        .execute_script("a", ScriptLanguage::Python, None)
        .unwrap();
    adapter
        .execute_script("b", ScriptLanguage::Python, None)
        .unwrap();
    adapter
        .execute_script("c", ScriptLanguage::Python, None)
        .unwrap();

    DccSceneInfo::get_scene_info(&adapter).unwrap();
    DccSnapshot::capture_viewport(&adapter, None, None, None, "png").unwrap();
    adapter.health_check().unwrap();

    assert_eq!(adapter.connect_count(), 2);
    assert_eq!(adapter.script_count(), 3);
    assert_eq!(adapter.scene_query_count(), 1);
    assert_eq!(adapter.snapshot_count(), 1);
    assert_eq!(adapter.health_check_count(), 1);
}

#[test]
fn test_reset_counters() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();
    adapter
        .execute_script("x", ScriptLanguage::Python, None)
        .unwrap();

    adapter.reset_counters();

    assert_eq!(adapter.connect_count(), 0);
    assert_eq!(adapter.script_count(), 0);
    assert_eq!(adapter.scene_query_count(), 0);
    assert_eq!(adapter.snapshot_count(), 0);
    assert_eq!(adapter.health_check_count(), 0);
    assert_eq!(adapter.disconnect_count(), 0);
}

#[test]
fn test_cross_protocol_counters() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    // DccSceneManager calls
    let sm = adapter.as_scene_manager().unwrap();
    let _ = sm.list_objects(None);
    let _ = sm.get_selection();
    assert_eq!(adapter.scene_manager_count(), 2);

    // DccTransform calls
    let tf = adapter.as_transform().unwrap();
    let _ = tf.get_transform("pCube1");
    assert_eq!(adapter.transform_count(), 1);

    // DccRenderCapture calls
    let rc = adapter.as_render_capture().unwrap();
    let _ = rc.get_render_settings();
    assert_eq!(adapter.render_capture_count(), 1);

    // DccHierarchy calls
    let hier = adapter.as_hierarchy().unwrap();
    let _ = hier.get_hierarchy();
    assert_eq!(adapter.hierarchy_count(), 1);
}
