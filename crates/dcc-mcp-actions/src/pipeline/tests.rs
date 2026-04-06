//! Tests for the action middleware pipeline.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::{Arc, Mutex};
use std::time::Duration;

use serde_json::json;

use crate::dispatcher::{ActionDispatcher, DispatchError, DispatchResult};
use crate::registry::{ActionMeta, ActionRegistry};

use super::{
    ActionMiddleware, ActionPipeline, AuditMiddleware, LoggingMiddleware, MiddlewareContext,
    RateLimitMiddleware, TimingMiddleware,
};

fn make_pipeline_with_echo() -> ActionPipeline {
    let registry = ActionRegistry::new();
    registry.register_action(ActionMeta {
        name: "echo".into(),
        dcc: "mock".into(),
        ..Default::default()
    });
    let dispatcher = ActionDispatcher::new(registry);
    dispatcher.register_handler("echo", Ok);
    ActionPipeline::new(dispatcher)
}

fn make_pipeline_with_failing() -> ActionPipeline {
    let registry = ActionRegistry::new();
    let dispatcher = ActionDispatcher::new(registry);
    dispatcher.register_handler("fail", |_| Err("intentional failure".to_string()));
    ActionPipeline::new(dispatcher)
}

// ── MiddlewareContext ────────────────────────────────────────────────────

mod context {
    use super::*;

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
}

// ── ActionPipeline basics ────────────────────────────────────────────────

mod pipeline {
    use super::*;

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
}

// ── LoggingMiddleware ────────────────────────────────────────────────────

mod logging {
    use super::*;

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
}

// ── TimingMiddleware ─────────────────────────────────────────────────────

mod timing {
    use super::*;

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
}

// ── RateLimitMiddleware ──────────────────────────────────────────────────

mod rate_limit {
    use super::*;

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
}

// ── AuditMiddleware ──────────────────────────────────────────────────────

mod audit {
    use super::*;

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
}

// ── Custom middleware ────────────────────────────────────────────────────

mod custom {
    use super::*;

    struct CountingMiddleware {
        before_count: Arc<AtomicUsize>,
        after_count: Arc<AtomicUsize>,
    }

    impl ActionMiddleware for CountingMiddleware {
        fn before_dispatch(&self, _ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
            self.before_count.fetch_add(1, Ordering::Relaxed);
            Ok(())
        }

        fn after_dispatch(
            &self,
            _ctx: &MiddlewareContext,
            _result: Result<&DispatchResult, &DispatchError>,
        ) {
            self.after_count.fetch_add(1, Ordering::Relaxed);
        }

        fn name(&self) -> &'static str {
            "counting"
        }
    }

    #[test]
    fn test_custom_middleware_called_on_success() {
        let before = Arc::new(AtomicUsize::new(0));
        let after = Arc::new(AtomicUsize::new(0));

        let mut pipeline = make_pipeline_with_echo();
        pipeline.add_middleware(CountingMiddleware {
            before_count: before.clone(),
            after_count: after.clone(),
        });

        pipeline.dispatch("echo", json!({})).unwrap();

        assert_eq!(before.load(Ordering::Relaxed), 1);
        assert_eq!(after.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_custom_middleware_called_on_failure() {
        let before = Arc::new(AtomicUsize::new(0));
        let after = Arc::new(AtomicUsize::new(0));

        let mut pipeline = make_pipeline_with_failing();
        pipeline.add_middleware(CountingMiddleware {
            before_count: before.clone(),
            after_count: after.clone(),
        });

        let _ = pipeline.dispatch("fail", json!({}));

        assert_eq!(before.load(Ordering::Relaxed), 1);
        assert_eq!(after.load(Ordering::Relaxed), 1);
    }

    #[test]
    fn test_multiple_middleware_order() {
        let calls: Arc<Mutex<Vec<String>>> = Arc::new(Mutex::new(Vec::new()));

        struct OrderMiddleware {
            id: &'static str,
            calls: Arc<Mutex<Vec<String>>>,
        }

        impl ActionMiddleware for OrderMiddleware {
            fn before_dispatch(&self, _ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("before:{}", self.id));
                Ok(())
            }

            fn after_dispatch(
                &self,
                _ctx: &MiddlewareContext,
                _result: Result<&DispatchResult, &DispatchError>,
            ) {
                self.calls
                    .lock()
                    .unwrap()
                    .push(format!("after:{}", self.id));
            }

            fn name(&self) -> &'static str {
                "order"
            }
        }

        let mut pipeline = make_pipeline_with_echo();
        pipeline.add_middleware(OrderMiddleware {
            id: "first",
            calls: calls.clone(),
        });
        pipeline.add_middleware(OrderMiddleware {
            id: "second",
            calls: calls.clone(),
        });

        pipeline.dispatch("echo", json!({})).unwrap();

        let log = calls.lock().unwrap().clone();
        assert_eq!(
            log,
            vec![
                "before:first",
                "before:second",
                "after:second",
                "after:first",
            ]
        );
    }

    #[test]
    fn test_middleware_abort_on_before_error() {
        let after_called = Arc::new(AtomicUsize::new(0));

        struct AbortMiddleware;

        impl ActionMiddleware for AbortMiddleware {
            fn before_dispatch(&self, _ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
                Err(DispatchError::HandlerError(
                    "aborted by middleware".to_string(),
                ))
            }

            fn name(&self) -> &'static str {
                "abort"
            }
        }

        struct TrackingMiddleware {
            count: Arc<AtomicUsize>,
        }

        impl ActionMiddleware for TrackingMiddleware {
            fn after_dispatch(
                &self,
                _ctx: &MiddlewareContext,
                _result: Result<&DispatchResult, &DispatchError>,
            ) {
                self.count.fetch_add(1, Ordering::Relaxed);
            }

            fn name(&self) -> &'static str {
                "tracking"
            }
        }

        let mut pipeline = make_pipeline_with_echo();
        pipeline.add_middleware(AbortMiddleware);
        pipeline.add_middleware(TrackingMiddleware {
            count: after_called.clone(),
        });

        let err = pipeline.dispatch("echo", json!({})).unwrap_err();
        assert!(err.to_string().contains("aborted by middleware"));
    }

    #[test]
    fn test_middleware_mutates_params() {
        struct DefaultParamMiddleware;

        impl ActionMiddleware for DefaultParamMiddleware {
            fn before_dispatch(&self, ctx: &mut MiddlewareContext) -> Result<(), DispatchError> {
                if ctx.params.is_object() {
                    ctx.params
                        .as_object_mut()
                        .unwrap()
                        .insert("injected".to_string(), json!("yes"));
                }
                Ok(())
            }

            fn name(&self) -> &'static str {
                "default_param"
            }
        }

        let mut pipeline = make_pipeline_with_echo();
        pipeline.add_middleware(DefaultParamMiddleware);

        let result = pipeline
            .dispatch("echo", json!({"original": "value"}))
            .unwrap();
        assert_eq!(result.output["injected"], json!("yes"));
        assert_eq!(result.output["original"], json!("value"));
    }
}
