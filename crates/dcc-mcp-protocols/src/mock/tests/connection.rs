use super::*;

#[test]
fn test_connect_disconnect() {
    let mut adapter = MockDccAdapter::new();
    assert!(!adapter.is_connected());

    adapter.connect().unwrap();
    assert!(adapter.is_connected());
    assert_eq!(adapter.connect_count(), 1);

    adapter.disconnect().unwrap();
    assert!(!adapter.is_connected());
    assert_eq!(adapter.disconnect_count(), 1);
}

#[test]
fn test_connect_failure() {
    let config = MockConfig::builder()
        .connect_should_fail("Test failure")
        .build();
    let mut adapter = MockDccAdapter::with_config(config);

    let result = adapter.connect();
    assert!(result.is_err());
    let err = result.unwrap_err();
    assert_eq!(err.code, DccErrorCode::ConnectionFailed);
    assert!(err.message.contains("Test failure"));
    assert!(err.recoverable);
    assert!(!adapter.is_connected());
}

#[test]
fn test_health_check_connected() {
    let mut adapter = MockDccAdapter::new();
    adapter.connect().unwrap();

    let rtt = adapter.health_check().unwrap();
    assert_eq!(rtt, 1); // default latency
    assert_eq!(adapter.health_check_count(), 1);
}

#[test]
fn test_health_check_disconnected() {
    let adapter = MockDccAdapter::new();
    let result = adapter.health_check();
    assert!(result.is_err());
}

#[test]
fn test_custom_health_check_latency() {
    let config = MockConfig::builder().health_check_latency_ms(42).build();
    let mut adapter = MockDccAdapter::with_config(config);
    adapter.connect().unwrap();

    assert_eq!(adapter.health_check().unwrap(), 42);
}
