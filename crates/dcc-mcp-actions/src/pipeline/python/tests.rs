//! Tests for the PyO3 pipeline bindings (Rust-level).
//!
//! These tests exercise the Rust `ActionPipeline` through the same shared
//! Arc wrappers used by the Python bindings, ensuring that middleware
//! implementations behave correctly at the Rust layer.

use std::sync::Arc;
use std::time::Duration;

use serde_json::json;

use crate::dispatcher::{ActionDispatcher, DispatchError};
use crate::pipeline::{
    ActionPipeline, AuditMiddleware, LoggingMiddleware, RateLimitMiddleware, TimingMiddleware,
};
use crate::registry::ActionMeta;
use crate::registry::ActionRegistry;

use super::{SharedAuditMiddleware, SharedRateLimitMiddleware, SharedTimingMiddleware};

fn make_pipeline() -> ActionPipeline {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "ping".into(),
        dcc: "mock".into(),
        ..Default::default()
    });
    let dispatcher = ActionDispatcher::new(reg);
    dispatcher.register_handler("ping", |_| Ok(json!("pong")));
    ActionPipeline::new(dispatcher)
}

#[test]
fn test_pipeline_no_middleware() {
    let pipeline = make_pipeline();
    assert_eq!(pipeline.middleware_count(), 0);
    let result = pipeline.dispatch("ping", json!({})).unwrap();
    assert_eq!(result.output, json!("pong"));
}

#[test]
fn test_pipeline_with_logging() {
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(LoggingMiddleware::new());
    assert_eq!(pipeline.middleware_count(), 1);
    let result = pipeline.dispatch("ping", json!({})).unwrap();
    assert_eq!(result.output, json!("pong"));
}

#[test]
fn test_pipeline_with_timing() {
    let timing = Arc::new(TimingMiddleware::new());
    let timing_clone = Arc::clone(&timing);
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(SharedTimingMiddleware(timing_clone));
    let result = pipeline.dispatch("ping", json!({})).unwrap();
    assert_eq!(result.output, json!("pong"));
    assert!(timing.last_elapsed("ping").is_some());
}

#[test]
fn test_pipeline_with_audit() {
    let audit = Arc::new(AuditMiddleware::new());
    let audit_clone = Arc::clone(&audit);
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(SharedAuditMiddleware(audit_clone));
    let _ = pipeline.dispatch("ping", json!({}));
    assert_eq!(audit.record_count(), 1);
    let records = audit.records();
    assert_eq!(records[0].action, "ping");
    assert!(records[0].success);
}

#[test]
fn test_pipeline_with_rate_limit_ok() {
    let rl = Arc::new(RateLimitMiddleware::new(5, Duration::from_secs(60)));
    let rl_clone = Arc::clone(&rl);
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(SharedRateLimitMiddleware(rl_clone));
    for _ in 0..5 {
        let r = pipeline.dispatch("ping", json!({}));
        assert!(r.is_ok());
    }
}

#[test]
fn test_pipeline_with_rate_limit_exceeded() {
    let rl = Arc::new(RateLimitMiddleware::new(2, Duration::from_secs(60)));
    let rl_clone = Arc::clone(&rl);
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(SharedRateLimitMiddleware(rl_clone));
    let _ = pipeline.dispatch("ping", json!({}));
    let _ = pipeline.dispatch("ping", json!({}));
    let result = pipeline.dispatch("ping", json!({}));
    assert!(result.is_err());
    match result.unwrap_err() {
        DispatchError::HandlerError(msg) => assert!(msg.contains("rate limit exceeded")),
        e => panic!("expected HandlerError, got {e:?}"),
    }
}

#[test]
fn test_pipeline_handler_not_found() {
    let pipeline = make_pipeline();
    let result = pipeline.dispatch("nonexistent", json!({}));
    assert!(result.is_err());
    assert!(matches!(
        result.unwrap_err(),
        DispatchError::HandlerNotFound(_)
    ));
}

#[test]
fn test_pipeline_middleware_names() {
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(LoggingMiddleware::new());
    pipeline.add_middleware(TimingMiddleware::new());
    assert_eq!(pipeline.middleware_names(), vec!["logging", "timing"]);
}

#[test]
fn test_audit_records_for_action() {
    let audit = Arc::new(AuditMiddleware::new());
    let audit_clone = Arc::clone(&audit);
    let mut pipeline = make_pipeline();
    pipeline.add_middleware(SharedAuditMiddleware(audit_clone));
    let _ = pipeline.dispatch("ping", json!({}));
    let records = audit.records_for_action("ping");
    assert_eq!(records.len(), 1);
    let empty = audit.records_for_action("missing");
    assert!(empty.is_empty());
}
