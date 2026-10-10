use super::*;

#[test]
fn test_default_adapter() {
    let adapter = MockDccAdapter::new();
    assert_eq!(adapter.info().dcc_type, "mock");
    assert_eq!(adapter.info().version, "1.0.0");
    assert!(adapter.info().python_version.is_some());
    assert!(!adapter.is_connected());
}

#[test]
fn test_custom_config() {
    let config = MockConfig::builder()
        .dcc_type("test_dcc")
        .version("2.0.0")
        .python_version("3.9.0")
        .platform("linux")
        .pid(42)
        .metadata("renderer", "cycles")
        .build();

    let adapter = MockDccAdapter::with_config(config);
    assert_eq!(adapter.info().dcc_type, "test_dcc");
    assert_eq!(adapter.info().version, "2.0.0");
    assert_eq!(adapter.info().python_version.as_deref(), Some("3.9.0"));
    assert_eq!(adapter.info().platform, "linux");
    assert_eq!(adapter.info().pid, 42);
    assert_eq!(adapter.info().metadata["renderer"], "cycles");
}

#[test]
fn test_no_python_config() {
    let config = MockConfig::builder().no_python().build();
    let adapter = MockDccAdapter::with_config(config);
    assert!(adapter.info().python_version.is_none());
}
