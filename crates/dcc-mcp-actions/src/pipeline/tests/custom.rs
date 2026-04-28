//! Custom middleware implementation tests.

use super::fixtures::{make_pipeline_with_echo, make_pipeline_with_failing};
use super::*;

// ── Custom middleware ────────────────────────────────────────────────────

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
            self.calls.lock().push(format!("before:{}", self.id));
            Ok(())
        }

        fn after_dispatch(
            &self,
            _ctx: &MiddlewareContext,
            _result: Result<&DispatchResult, &DispatchError>,
        ) {
            self.calls.lock().push(format!("after:{}", self.id));
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

    let log = calls.lock().clone();
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
