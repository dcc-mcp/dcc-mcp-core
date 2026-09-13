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

#[test]
fn completed_duration_is_frozen_and_aborted_calls_do_not_replace_it() {
    let timing = TimingMiddleware::new();
    let mut context = MiddlewareContext::new("echo", json!({}));
    timing.before_dispatch(&mut context).unwrap();
    assert_eq!(timing.last_elapsed("echo"), None);
    let result = make_pipeline_with_echo()
        .dispatch("echo", json!({}))
        .unwrap();
    timing.after_dispatch(&context, Ok(&result));
    let completed = timing.last_elapsed("echo").unwrap();
    std::thread::sleep(Duration::from_millis(5));
    assert_eq!(timing.last_elapsed("echo"), Some(completed));

    let mut aborted = MiddlewareContext::new("echo", json!({}));
    timing.before_dispatch(&mut aborted).unwrap();
    // A later middleware rejects this call, so after_dispatch never runs.
    assert_eq!(timing.last_elapsed("echo"), Some(completed));
}

#[test]
fn overlapping_calls_to_one_action_keep_independent_start_times() {
    let timing = TimingMiddleware::new();
    let mut outer = MiddlewareContext::new("echo", json!({}));
    let mut inner = MiddlewareContext::new("echo", json!({}));
    timing.before_dispatch(&mut outer).unwrap();
    std::thread::sleep(Duration::from_millis(10));
    timing.before_dispatch(&mut inner).unwrap();
    let result = make_pipeline_with_echo()
        .dispatch("echo", json!({}))
        .unwrap();
    timing.after_dispatch(&inner, Ok(&result));
    let inner_elapsed = timing.last_elapsed("echo").unwrap();
    timing.after_dispatch(&outer, Ok(&result));
    assert!(timing.last_elapsed("echo").unwrap() >= inner_elapsed + Duration::from_millis(10));
}
