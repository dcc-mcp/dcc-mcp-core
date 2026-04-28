//! Basic ActionPipeline tests.

use super::fixtures::make_pipeline_with_echo;
use super::*;

// ── ActionPipeline basics ────────────────────────────────────────────────

#[test]
fn test_pipeline_no_middleware_dispatch() {
    let pipeline = make_pipeline_with_echo();
    let result = pipeline.dispatch("echo", json!({"msg": "hello"})).unwrap();
    assert_eq!(result.output, json!({"msg": "hello"}));
}

#[test]
fn test_pipeline_middleware_count() {
    let mut pipeline = make_pipeline_with_echo();
    assert_eq!(pipeline.middleware_count(), 0);
    pipeline.add_middleware(LoggingMiddleware::new());
    assert_eq!(pipeline.middleware_count(), 1);
    pipeline.add_middleware(TimingMiddleware::new());
    assert_eq!(pipeline.middleware_count(), 2);
}

#[test]
fn test_pipeline_middleware_names() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(LoggingMiddleware::new());
    pipeline.add_middleware(TimingMiddleware::new());
    pipeline.add_middleware(AuditMiddleware::new());

    let names = pipeline.middleware_names();
    assert_eq!(names, vec!["logging", "timing", "audit"]);
}

#[test]
fn test_pipeline_dispatch_not_found() {
    let pipeline = make_pipeline_with_echo();
    let err = pipeline.dispatch("nonexistent", json!({})).unwrap_err();
    assert!(matches!(err, DispatchError::HandlerNotFound(_)));
}

#[test]
fn test_pipeline_access_dispatcher() {
    let pipeline = make_pipeline_with_echo();
    assert!(pipeline.dispatcher().has_handler("echo"));
}
