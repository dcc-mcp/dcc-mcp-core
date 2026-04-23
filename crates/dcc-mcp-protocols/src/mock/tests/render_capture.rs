use super::*;
use std::collections::HashMap;

fn connected_adapter() -> MockDccAdapter {
    let mut a = MockDccAdapter::new();
    a.connect().unwrap();
    a
}

#[test]
fn test_capture_viewport_via_render_capture_trait() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();

    let result = rc
        .capture_viewport(Some("top"), Some(1280), Some(720), "jpeg")
        .unwrap();
    assert_eq!(result.width, 1280);
    assert_eq!(result.height, 720);
    assert_eq!(result.format, "jpeg");
    assert_eq!(result.viewport.as_deref(), Some("top"));
    assert!(!result.data.is_empty());
}

#[test]
fn test_render_scene() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();

    let out = rc
        .render_scene(
            "/renders/frame001.png",
            Some(1920),
            Some(1080),
            Some("arnold"),
        )
        .unwrap();
    assert_eq!(out.file_path, "/renders/frame001.png");
    assert_eq!(out.width, 1920);
    assert_eq!(out.height, 1080);
    assert_eq!(out.format, "png");
    assert_eq!(out.render_time_ms, 100); // default
}

#[test]
fn test_render_scene_uses_settings_resolution() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();

    // Use None for resolution — should fall back to render settings (1920x1080)
    let out = rc.render_scene("/out/frame.exr", None, None, None).unwrap();
    assert_eq!(out.width, 1920);
    assert_eq!(out.height, 1080);
    assert_eq!(out.format, "exr");
}

#[test]
fn test_get_render_settings() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();

    let settings = rc.get_render_settings().unwrap();
    assert_eq!(settings["width"], "1920");
    assert_eq!(settings["height"], "1080");
    assert_eq!(settings["renderer"], "default");
}

#[test]
fn test_set_render_settings_partial_update() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();

    let mut updates = HashMap::new();
    updates.insert("renderer".to_string(), "arnold".to_string());
    updates.insert("samples".to_string(), "256".to_string());
    rc.set_render_settings(updates).unwrap();

    let settings = rc.get_render_settings().unwrap();
    assert_eq!(settings["renderer"], "arnold");
    assert_eq!(settings["samples"], "256");
    // Other keys should remain untouched
    assert_eq!(settings["width"], "1920");
}

#[test]
fn test_render_capture_not_exposed_when_snapshot_disabled() {
    let config = MockConfig::builder().snapshot_enabled(false).build();
    let adapter = MockDccAdapter::with_config(config);
    // as_render_capture returns None when snapshot_enabled is false
    assert!(adapter.as_render_capture().is_none());
}

#[test]
fn test_render_capture_not_connected() {
    let adapter = MockDccAdapter::new();
    let rc = adapter.as_render_capture().unwrap();
    assert!(rc.get_render_settings().is_err());
    assert!(rc.render_scene("/out/x.png", None, None, None).is_err());
}

#[test]
fn test_render_counter_increments() {
    let adapter = connected_adapter();
    let rc = adapter.as_render_capture().unwrap();
    rc.capture_viewport(None, None, None, "png").unwrap();
    rc.render_scene("/x.png", None, None, None).unwrap();
    rc.get_render_settings().unwrap();
    assert_eq!(adapter.render_capture_count(), 3);
}
