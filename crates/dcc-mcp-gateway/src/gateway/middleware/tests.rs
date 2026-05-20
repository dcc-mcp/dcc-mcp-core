//! Tests for the middleware chain (issue #770).

use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::gateway::admin::trace::AgentContext;

use super::audit::{AuditEntry, AuditMiddleware, AuditSink};
use super::chain::MiddlewareChain;
use super::context::{CallContext, CallResult};
use super::error::MiddlewareError;
use super::quota::QuotaMiddleware;
use super::redaction::RedactionMiddleware;
use super::traits::{AfterCallMiddleware, BeforeCallMiddleware, MiddlewareFuture};

// ── Helpers ──────────────────────────────────────────────────────────────────

fn make_ctx(tool: &str) -> CallContext {
    CallContext::new("tools/call", "req-1", json!({})).with_tool_slug(tool)
}

// ── Ordering ─────────────────────────────────────────────────────────────────

struct AppendBefore(String, Arc<Mutex<Vec<String>>>);

impl BeforeCallMiddleware for AppendBefore {
    fn before_call<'a>(&'a self, ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
        let tag = self.0.clone();
        let log = self.1.clone();
        ctx.metadata.insert(format!("order_{}", &tag), tag.clone());
        Box::pin(async move {
            log.lock().unwrap().push(tag);
            Ok(())
        })
    }
}

struct AppendAfter(String, Arc<Mutex<Vec<String>>>);

impl AfterCallMiddleware for AppendAfter {
    fn after_call<'a>(
        &'a self,
        _ctx: &'a CallContext,
        _result: &'a mut CallResult,
    ) -> MiddlewareFuture<'a, ()> {
        let tag = self.0.clone();
        let log = self.1.clone();
        Box::pin(async move {
            log.lock().unwrap().push(tag);
            Ok(())
        })
    }
}

#[tokio::test]
async fn test_chain_before_executes_in_order() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));

    let chain = MiddlewareChain::new()
        .with_before(Arc::new(AppendBefore("A".into(), log.clone())))
        .with_before(Arc::new(AppendBefore("B".into(), log.clone())))
        .with_before(Arc::new(AppendBefore("C".into(), log.clone())));

    let mut ctx = make_ctx("search_tools");
    chain.run_before(&mut ctx).await.unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["A", "B", "C"]);
}

#[tokio::test]
async fn test_chain_after_executes_in_order() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));

    let chain = MiddlewareChain::new()
        .with_after(Arc::new(AppendAfter("X".into(), log.clone())))
        .with_after(Arc::new(AppendAfter("Y".into(), log.clone())));

    let ctx = make_ctx("call_tool");
    let mut result = CallResult::from_tuple("ok", false);
    chain.run_after(&ctx, &mut result).await.unwrap();

    assert_eq!(*log.lock().unwrap(), vec!["X", "Y"]);
}

#[tokio::test]
async fn test_chain_aborts_on_first_before_error() {
    let log = Arc::new(Mutex::new(Vec::<String>::new()));

    struct Abort;
    impl BeforeCallMiddleware for Abort {
        fn before_call<'a>(&'a self, _ctx: &'a mut CallContext) -> MiddlewareFuture<'a, ()> {
            Box::pin(async move { Err(MiddlewareError::PolicyViolation("blocked".into())) })
        }
    }

    let chain = MiddlewareChain::new()
        .with_before(Arc::new(AppendBefore("first".into(), log.clone())))
        .with_before(Arc::new(Abort))
        .with_before(Arc::new(AppendBefore("should_not_run".into(), log.clone())));

    let mut ctx = make_ctx("call_tool");
    let err = chain.run_before(&mut ctx).await.unwrap_err();

    assert!(matches!(err, MiddlewareError::PolicyViolation(_)));
    // Only "first" should have run before the abort.
    assert_eq!(*log.lock().unwrap(), vec!["first"]);
}

// ── AuditMiddleware ───────────────────────────────────────────────────────────

#[derive(Default)]
struct SpySink(Arc<Mutex<Vec<AuditEntry>>>);

impl AuditSink for SpySink {
    fn record(&self, entry: AuditEntry) {
        self.0.lock().unwrap().push(entry);
    }
}

#[tokio::test]
async fn test_audit_middleware_records_call() {
    let spy = Arc::new(SpySink::default());
    let entries = spy.0.clone();
    let m = AuditMiddleware::new(spy);

    let mut ctx = CallContext::new("tools/call", "req-42", json!({"x": 1}))
        .with_tool_slug("call_tool")
        .with_session_id("sess-1");

    // BeforeCall stamps start time.
    m.before_call(&mut ctx).await.unwrap();
    assert!(ctx.metadata.contains_key("audit.start_time_ns"));

    // AfterCall writes the entry to the sink.
    let mut result = CallResult::from_tuple("all good", false);
    m.after_call(&ctx, &mut result).await.unwrap();

    let log = entries.lock().unwrap();
    assert_eq!(log.len(), 1);
    let e = &log[0];
    assert_eq!(e.method, "tools/call");
    assert_eq!(e.tool_slug.as_deref(), Some("call_tool"));
    assert_eq!(e.session_id.as_deref(), Some("sess-1"));
    assert_eq!(e.request_id, "req-42");
    assert!(!e.is_error);
    assert!(e.result_preview.contains("all good"));
}

// ── QuotaMiddleware ───────────────────────────────────────────────────────────

#[tokio::test]
async fn test_quota_middleware_allows_under_limit() {
    let m = QuotaMiddleware::new(5).with_window(Duration::from_secs(60));

    for i in 0..5u64 {
        let mut ctx = CallContext::new("tools/call", format!("req-{i}"), json!({}))
            .with_session_id("sess-quota");
        m.before_call(&mut ctx).await.unwrap_or_else(|_| {
            panic!("call {i} should be allowed");
        });
    }
}

#[tokio::test]
async fn test_quota_middleware_rejects_over_limit() {
    let m = QuotaMiddleware::new(3).with_window(Duration::from_secs(60));

    for i in 0..3u64 {
        let mut ctx = CallContext::new("tools/call", format!("req-{i}"), json!({}))
            .with_session_id("sess-quota-over");
        m.before_call(&mut ctx).await.unwrap();
    }

    // 4th call should be rejected.
    let mut ctx =
        CallContext::new("tools/call", "req-over", json!({})).with_session_id("sess-quota-over");
    let err = m.before_call(&mut ctx).await.unwrap_err();
    assert!(
        matches!(err, MiddlewareError::QuotaExceeded(_)),
        "expected QuotaExceeded, got {err:?}"
    );
}

#[tokio::test]
async fn test_quota_middleware_resets_after_window() {
    let m = QuotaMiddleware::new(2).with_window(Duration::from_millis(50));

    for i in 0..2u64 {
        let mut ctx = CallContext::new("tools/call", format!("req-{i}"), json!({}))
            .with_session_id("sess-reset");
        m.before_call(&mut ctx).await.unwrap();
    }

    // Wait for the window to expire.
    tokio::time::sleep(Duration::from_millis(60)).await;

    // Should be allowed again.
    let mut ctx =
        CallContext::new("tools/call", "req-new-window", json!({})).with_session_id("sess-reset");
    m.before_call(&mut ctx)
        .await
        .expect("call after window reset should succeed");
}

// ── RedactionMiddleware ───────────────────────────────────────────────────────

#[tokio::test]
async fn test_redaction_replaces_matching_fields() {
    let m = RedactionMiddleware::new(vec!["api_key", "token"]);

    let mut ctx = CallContext::new(
        "tools/call",
        "req-r",
        json!({
            "api_key": "secret-123",
            "name": "my-tool",
            "nested": {"token": "tok-456", "keep": "value"}
        }),
    );

    m.before_call(&mut ctx).await.unwrap();

    assert_eq!(ctx.args["api_key"].as_str(), Some("[REDACTED]"));
    assert_eq!(ctx.args["name"].as_str(), Some("my-tool"));
    assert_eq!(ctx.args["nested"]["token"].as_str(), Some("[REDACTED]"));
    assert_eq!(ctx.args["nested"]["keep"].as_str(), Some("value"));
}

#[tokio::test]
async fn test_redaction_handles_arrays() {
    let m = RedactionMiddleware::new(vec!["password"]);

    let mut ctx = CallContext::new(
        "tools/call",
        "req-arr",
        json!({
            "users": [
                {"name": "alice", "password": "pw1"},
                {"name": "bob", "password": "pw2"}
            ]
        }),
    );

    m.before_call(&mut ctx).await.unwrap();

    assert_eq!(
        ctx.args["users"][0]["password"].as_str(),
        Some("[REDACTED]")
    );
    assert_eq!(
        ctx.args["users"][1]["password"].as_str(),
        Some("[REDACTED]")
    );
    assert_eq!(ctx.args["users"][0]["name"].as_str(), Some("alice"));
}

#[tokio::test]
async fn test_redaction_noop_on_no_match() {
    let m = RedactionMiddleware::new(vec!["secret"]);

    let original = json!({"a": 1, "b": "hello"});
    let mut ctx = CallContext::new("tools/call", "req-noop", original.clone());
    m.before_call(&mut ctx).await.unwrap();

    assert_eq!(ctx.args, original);
}

// ── Integration: combined chain ───────────────────────────────────────────────

#[tokio::test]
async fn test_combined_chain_audit_quota_redaction() {
    let spy = Arc::new(SpySink::default());
    let entries = spy.0.clone();

    let chain = MiddlewareChain::new()
        .with_before(Arc::new(AuditMiddleware::new(spy.clone())))
        .with_before(Arc::new(QuotaMiddleware::new(10)))
        .with_before(Arc::new(RedactionMiddleware::new(vec!["token"])))
        .with_after(Arc::new(AuditMiddleware::new(spy)));

    let mut ctx = CallContext::new(
        "tools/call",
        "combined-1",
        json!({"tool": "run", "token": "secret"}),
    )
    .with_session_id("sess-combined")
    .with_transport("mcp")
    .with_agent_context(Some(AgentContext {
        agent_id: Some("agent-combined".to_string()),
        reasoning_summary: Some("Verify middleware telemetry fields.".to_string()),
        ..Default::default()
    }));

    chain.run_before(&mut ctx).await.unwrap();

    // Token should be redacted.
    assert_eq!(ctx.args["token"].as_str(), Some("[REDACTED]"));

    let mut result = CallResult::from_tuple("success", false);
    chain.run_after(&ctx, &mut result).await.unwrap();

    // AfterCall audit entry should have been written.
    let log = entries.lock().unwrap();
    assert!(!log.is_empty(), "at least one audit entry expected");
    assert_eq!(log[0].transport.as_deref(), Some("mcp"));
    assert_eq!(
        log[0]
            .agent_context
            .as_ref()
            .and_then(|ctx| ctx.agent_id.as_deref()),
        Some("agent-combined")
    );
}
