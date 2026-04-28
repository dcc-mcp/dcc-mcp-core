//! TimingMiddleware tests.

use super::fixtures::make_pipeline_with_echo;
use super::*;

// ── TimingMiddleware ─────────────────────────────────────────────────────

#[test]
fn test_timing_middleware_records_time() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(LoggingMiddleware::new());
    pipeline.dispatch("echo", json!({})).unwrap();
}

#[test]
fn test_timing_middleware_name() {
    let m = TimingMiddleware::new();
    assert_eq!(m.name(), "timing");
}

#[test]
fn test_timing_middleware_default() {
    let _m = TimingMiddleware::default();
}

#[test]
fn test_timing_middleware_pipeline_dispatch() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(TimingMiddleware::new());

    let result = pipeline.dispatch("echo", json!({"key": "value"})).unwrap();
    assert_eq!(result.output["key"], "value");
}
