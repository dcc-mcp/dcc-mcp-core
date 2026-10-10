use super::*;

#[test]
fn test_capture_viewport() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let result =
        DccSnapshot::capture_viewport(&adapter, Some("persp"), Some(800), Some(600), "png")
            .unwrap();

    assert_eq!(result.width, 800);
    assert_eq!(result.height, 600);
    assert_eq!(result.format, "png");
    assert_eq!(result.viewport.as_deref(), Some("persp"));
    assert!(!result.data.is_empty());
    assert_eq!(adapter.snapshot_count(), 1);
}

#[test]
fn test_capture_default_resolution() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png").unwrap();
    assert_eq!(result.width, 1920);
    assert_eq!(result.height, 1080);
}

#[test]
fn test_snapshot_disabled() {
    let config = MockConfig::builder().snapshot_enabled(false).build();
    let mut adapter = MockDccAdapter::with_config(config);
    adapter.connect().unwrap();

    let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png");
    assert!(result.is_err());
    assert_eq!(result.unwrap_err().code, DccErrorCode::Unsupported);
}

#[test]
fn test_snapshot_not_connected() {
    let adapter = MockDccAdapter::new();
    let result = DccSnapshot::capture_viewport(&adapter, None, None, None, "png");
    assert!(result.is_err());
}
