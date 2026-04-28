//! AuditMiddleware tests.

use super::*;

// ── AuditMiddleware ──────────────────────────────────────────────────────

#[test]
fn test_audit_records_success() {
    let audit = AuditMiddleware::new();

    let ctx = MiddlewareContext::new("create_sphere", json!({"radius": 1.0}));
    let fake_result = DispatchResult {
        action: "create_sphere".to_string(),
        output: json!({"created": true}),
        validation_skipped: false,
    };
    audit.after_dispatch(&ctx, Ok(&fake_result));

    assert_eq!(audit.record_count(), 1);
    let records = audit.records();
    assert_eq!(records[0].action, "create_sphere");
    assert!(records[0].success);
    assert!(records[0].error.is_none());
    assert!(records[0].output_preview.is_some());
}

#[test]
fn test_audit_records_failure() {
    let audit = AuditMiddleware::new();
    let ctx = MiddlewareContext::new("broken_action", json!({}));
    let err = DispatchError::HandlerError("something exploded".to_string());
    audit.after_dispatch(&ctx, Err(&err));

    assert_eq!(audit.record_count(), 1);
    let records = audit.records();
    assert!(!records[0].success);
    assert!(records[0].error.as_deref().unwrap().contains("exploded"));
}

#[test]
fn test_audit_pipeline_integration() {
    let audit = AuditMiddleware::new();
    let ctx = MiddlewareContext::new("ping", json!({}));
    let ok_result = DispatchResult {
        action: "ping".to_string(),
        output: json!("pong"),
        validation_skipped: true,
    };
    audit.after_dispatch(&ctx, Ok(&ok_result));

    let records = audit.records_for_action("ping");
    assert_eq!(records.len(), 1);
    assert!(records[0].success);
}

#[test]
fn test_audit_records_for_action_filter() {
    let audit = AuditMiddleware::new();

    for action in &["a", "b", "a", "c", "a"] {
        let ctx = MiddlewareContext::new(*action, json!(null));
        let result = DispatchResult {
            action: (*action).to_string(),
            output: json!(null),
            validation_skipped: true,
        };
        audit.after_dispatch(&ctx, Ok(&result));
    }

    assert_eq!(audit.records_for_action("a").len(), 3);
    assert_eq!(audit.records_for_action("b").len(), 1);
    assert_eq!(audit.records_for_action("c").len(), 1);
    assert_eq!(audit.records_for_action("missing").len(), 0);
}

#[test]
fn test_audit_clear() {
    let audit = AuditMiddleware::new();
    let ctx = MiddlewareContext::new("x", json!(null));
    let result = DispatchResult {
        action: "x".to_string(),
        output: json!(null),
        validation_skipped: true,
    };
    audit.after_dispatch(&ctx, Ok(&result));
    assert_eq!(audit.record_count(), 1);

    audit.clear();
    assert_eq!(audit.record_count(), 0);
}

#[test]
fn test_audit_output_preview_truncated() {
    let audit = AuditMiddleware::new();
    let ctx = MiddlewareContext::new("large", json!(null));
    let large_output: String = "x".repeat(500);
    let result = DispatchResult {
        action: "large".to_string(),
        output: json!(large_output),
        validation_skipped: true,
    };
    audit.after_dispatch(&ctx, Ok(&result));

    let records = audit.records();
    let preview = records[0].output_preview.as_deref().unwrap();
    assert!(preview.len() <= 260); // 256 + "..."
    assert!(preview.ends_with("..."));
}

#[test]
fn test_audit_no_params_recording() {
    let mut audit = AuditMiddleware::new();
    audit.record_params = false;

    let ctx = MiddlewareContext::new("action", json!({"secret": "token123"}));
    let result = DispatchResult {
        action: "action".to_string(),
        output: json!("ok"),
        validation_skipped: true,
    };
    audit.after_dispatch(&ctx, Ok(&result));

    let records = audit.records();
    assert_eq!(records[0].params, serde_json::Value::Null);
}

#[test]
fn test_audit_middleware_name() {
    let m = AuditMiddleware::new();
    assert_eq!(m.name(), "audit");
}

#[test]
fn test_audit_middleware_default() {
    let m = AuditMiddleware::default();
    assert!(m.record_params);
}
