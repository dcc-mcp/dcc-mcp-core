//! Tests for MiddlewareContext.

use super::*;

// ── MiddlewareContext ────────────────────────────────────────────────────

#[test]
fn test_context_new() {
    let ctx = MiddlewareContext::new("my_action", json!({"x": 1}));
    assert_eq!(ctx.action, "my_action");
    assert_eq!(ctx.params, json!({"x": 1}));
    assert!(ctx.extensions.is_empty());
}

#[test]
fn test_context_insert_get() {
    let mut ctx = MiddlewareContext::new("a", json!(null));
    ctx.insert("key", json!(42));
    assert_eq!(ctx.get("key"), Some(&json!(42)));
    assert!(ctx.get("missing").is_none());
}

#[test]
fn test_context_overwrite() {
    let mut ctx = MiddlewareContext::new("a", json!(null));
    ctx.insert("k", json!(1));
    ctx.insert("k", json!(2));
    assert_eq!(ctx.get("k"), Some(&json!(2)));
}
