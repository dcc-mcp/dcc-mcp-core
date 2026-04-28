//! RateLimitMiddleware tests.

use super::fixtures::make_pipeline_with_echo;
use super::*;

// ── RateLimitMiddleware ──────────────────────────────────────────────────

#[test]
fn test_rate_limit_allows_under_limit() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(RateLimitMiddleware::new(5, Duration::from_secs(60)));

    for _ in 0..5 {
        pipeline.dispatch("echo", json!({})).unwrap();
    }
}

#[test]
fn test_rate_limit_blocks_over_limit() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(RateLimitMiddleware::new(2, Duration::from_secs(60)));

    pipeline.dispatch("echo", json!({})).unwrap();
    pipeline.dispatch("echo", json!({})).unwrap();

    let err = pipeline.dispatch("echo", json!({})).unwrap_err();
    assert!(matches!(err, DispatchError::HandlerError(_)));
    assert!(err.to_string().contains("rate limit exceeded"));
}

#[test]
fn test_rate_limit_independent_per_action() {
    let registry = ActionRegistry::new();
    for name in &["action_a", "action_b"] {
        registry.register_action(ActionMeta {
            name: (*name).into(),
            dcc: "mock".into(),
            ..Default::default()
        });
    }
    let dispatcher = ActionDispatcher::new(registry);
    dispatcher.register_handler("action_a", |_| Ok(json!("a")));
    dispatcher.register_handler("action_b", |_| Ok(json!("b")));

    let mut pipeline = ActionPipeline::new(dispatcher);
    pipeline.add_middleware(RateLimitMiddleware::new(1, Duration::from_secs(60)));

    pipeline.dispatch("action_a", json!({})).unwrap();
    pipeline.dispatch("action_b", json!({})).unwrap();

    let err_a = pipeline.dispatch("action_a", json!({})).unwrap_err();
    assert!(err_a.to_string().contains("rate limit exceeded"));
}

#[test]
fn test_rate_limit_window_reset() {
    let mut pipeline = make_pipeline_with_echo();
    pipeline.add_middleware(RateLimitMiddleware::new(1, Duration::from_nanos(1)));

    pipeline.dispatch("echo", json!({})).unwrap();

    std::thread::sleep(Duration::from_millis(1));

    pipeline.dispatch("echo", json!({})).unwrap();
}

#[test]
fn test_rate_limit_middleware_name() {
    let m = RateLimitMiddleware::new(10, Duration::from_secs(1));
    assert_eq!(m.name(), "rate_limit");
}

#[test]
fn test_rate_limit_call_count() {
    let rl_direct = RateLimitMiddleware::new(10, Duration::from_secs(60));
    assert_eq!(rl_direct.call_count("echo"), 0);
}
