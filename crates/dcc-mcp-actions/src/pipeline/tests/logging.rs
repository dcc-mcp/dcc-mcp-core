//! LoggingMiddleware tests.

use super::fixtures::{make_pipeline_with_echo, make_pipeline_with_failing};
use super::*;

// ── LoggingMiddleware ────────────────────────────────────────────────────

#[test]
fn test_logging_middleware_success() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(LoggingMiddleware::new());

    let result = pipeline.dispatch("echo", json!({"v": 1})).unwrap();
    assert_eq!(result.output["v"], 1);
}

#[test]
fn test_logging_middleware_with_params() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(LoggingMiddleware::with_params());

    let result = pipeline.dispatch("echo", json!({"v": 99})).unwrap();
    assert_eq!(result.output["v"], 99);
}

#[test]
fn test_logging_middleware_on_failure() {
    let mut pipeline = make_pipeline_with_failing();
    pipeline.add_middleware(LoggingMiddleware::new());

    let err = pipeline.dispatch("fail", json!({})).unwrap_err();
    assert!(matches!(err, DispatchError::HandlerError(_)));
}

#[test]
fn test_logging_middleware_name() {
    let m = LoggingMiddleware::new();
    assert_eq!(m.name(), "logging");
}

#[test]
fn test_logging_middleware_default() {
    let m = LoggingMiddleware::default();
    assert!(!m.log_params);
}
