//! Unit tests for [`ActionChain`](super::chain::ActionChain).

use super::exec::ActionChain;
use super::interpolate_impl::{context, interpolate};
use super::types::ErrorAction;
use crate::dispatcher::ActionDispatcher;
use crate::registry::{ActionMeta, ActionRegistry};
use serde_json::{Value, json};

fn make_dispatcher() -> ActionDispatcher {
    let reg = ActionRegistry::new();
    ActionDispatcher::new(reg)
}

fn register(dispatcher: &ActionDispatcher, name: &'static str, output: Value) {
    dispatcher.register_handler(name, move |_| Ok(output.clone()));
}

// ── basic ──────────────────────────────────────────────────────────────────

#[test]
fn test_single_step_success() {
    let d = make_dispatcher();
    register(&d, "ping", json!({"pong": true}));

    let result = ActionChain::new()
        .step("ping", json!({}))
        .run(&d, json!({}))
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps.len(), 1);
    assert_eq!(result.steps[0].output, json!({"pong": true}));
}

#[test]
fn test_two_steps_context_propagation() {
    let d = make_dispatcher();
    // step_a outputs {value: 99}; step_b receives it via interpolation
    register(&d, "step_a", json!({"value": 99}));
    d.register_handler("step_b", |params| Ok(json!({"received": params["value"]})));

    let result = ActionChain::new()
        .step("step_a", json!({}))
        .step("step_b", json!({"value": "{value}"}))
        .run(&d, json!({}))
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps[1].output, json!({"received": 99}));
}

#[test]
fn test_initial_context_available() {
    let d = make_dispatcher();
    d.register_handler("use_ctx", |params| {
        Ok(json!({"path": params["export_path"]}))
    });

    let result = ActionChain::new()
        .step("use_ctx", json!({"export_path": "{export_path}"}))
        .run(&d, json!({"export_path": "/tmp/out.fbx"}))
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps[0].output["path"], json!("/tmp/out.fbx"));
}

// ── error handling ────────────────────────────────────────────────────────

#[test]
fn test_step_failure_aborts_by_default() {
    let d = make_dispatcher();
    register(&d, "ok_step", json!({}));
    // "bad_step" has no handler — will fail with HandlerNotFound

    let result = ActionChain::new()
        .step("bad_step", json!({}))
        .step("ok_step", json!({}))
        .run(&d, json!({}))
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.aborted_at, Some(0));
    assert_eq!(result.steps.len(), 1); // second step never ran
}

#[test]
fn test_continue_on_failure() {
    let d = make_dispatcher();
    register(&d, "ok_step", json!({"ran": true}));

    let result = ActionChain::new()
        .step("missing_action", json!({}))
        .continue_on_failure()
        .step("ok_step", json!({}))
        .run(&d, json!({}))
        .unwrap();

    // Chain didn't abort; both steps ran
    assert_eq!(result.steps.len(), 2);
    assert!(!result.steps[0].success);
    assert!(result.steps[1].success);
    // Overall success is true because abort never triggered
    assert!(result.success);
}

#[test]
fn test_on_error_abort() {
    let d = make_dispatcher();
    register(&d, "ok_step", json!({}));

    let result = ActionChain::new()
        .step("missing", json!({}))
        .step("ok_step", json!({}))
        .on_error(|_, _| ErrorAction::Abort)
        .run(&d, json!({}))
        .unwrap();

    assert!(!result.success);
    assert_eq!(result.aborted_at, Some(0));
}

#[test]
fn test_on_error_continue() {
    let d = make_dispatcher();
    register(&d, "ok_step", json!({"ran": true}));

    let result = ActionChain::new()
        .step("missing", json!({}))
        .step("ok_step", json!({}))
        .on_error(|_, _| ErrorAction::Continue)
        .run(&d, json!({}))
        .unwrap();

    assert_eq!(result.steps.len(), 2);
    assert!(result.steps[1].success);
    assert!(result.success);
}

// ── dynamic steps ─────────────────────────────────────────────────────────

#[test]
fn test_step_with_closure() {
    let d = make_dispatcher();
    register(&d, "step_a", json!({"items": ["a", "b", "c"]}));
    d.register_handler("step_b", |params: Value| {
        let count = params["items"].as_array().map(|a| a.len()).unwrap_or(0);
        Ok(json!({"count": count}))
    });

    let result = ActionChain::new()
        .step("step_a", json!({}))
        .step_with(
            "step_b",
            |ctx| json!({"items": ctx.get("items").cloned().unwrap_or(json!([]))}),
        )
        .run(&d, json!({}))
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps[1].output["count"], json!(3));
}

// ── empty chain error ─────────────────────────────────────────────────────

#[test]
fn test_empty_chain_returns_error() {
    let d = make_dispatcher();
    let err = ActionChain::new().run(&d, json!({})).unwrap_err();
    assert!(err.contains("no steps"));
}

// ── interpolation ─────────────────────────────────────────────────────────

#[test]
fn test_interpolate_whole_placeholder_preserves_type() {
    let ctx = json!({"count": 42});
    let v = interpolate(&json!("{count}"), &ctx);
    assert_eq!(v, json!(42));
}

#[test]
fn test_interpolate_inline_becomes_string() {
    let ctx = json!({"name": "world"});
    let v = interpolate(&json!("hello {name}!"), &ctx);
    assert_eq!(v, json!("hello world!"));
}

#[test]
fn test_interpolate_missing_key_unchanged() {
    let ctx = json!({});
    let v = interpolate(&json!("{missing}"), &ctx);
    assert_eq!(v, json!("{missing}"));
}

#[test]
fn test_interpolate_nested_object() {
    let ctx = json!({"prefix": "char_"});
    let v = interpolate(&json!({"name": "{prefix}hero"}), &ctx);
    assert_eq!(v, json!({"name": "char_hero"}));
}

// ── context helper ────────────────────────────────────────────────────────

#[test]
fn test_context_helper() {
    let ctx = context([("key", "val"), ("num", "99")]);
    assert_eq!(ctx["key"], json!("val"));
    assert_eq!(ctx["num"], json!("99"));
}

// ── label / message ───────────────────────────────────────────────────────

#[test]
fn test_label_appears_in_result() {
    let d = make_dispatcher();
    register(&d, "my_action", json!({}));

    let result = ActionChain::new()
        .step("my_action", json!({}))
        .label("Do the thing")
        .run(&d, json!({}))
        .unwrap();

    assert_eq!(result.steps[0].label, "Do the thing");
}

#[test]
fn test_message_on_success() {
    let d = make_dispatcher();
    register(&d, "a", json!({}));
    register(&d, "b", json!({}));

    let result = ActionChain::new()
        .step("a", json!({}))
        .step("b", json!({}))
        .run(&d, json!({}))
        .unwrap();

    assert!(result.message.contains("2/2"));
}

#[test]
fn test_message_on_abort() {
    let d = make_dispatcher();

    let result = ActionChain::new()
        .step("missing", json!({}))
        .run(&d, json!({}))
        .unwrap();

    assert!(!result.success);
    assert!(result.message.contains("aborted"));
}

// ── registry integration ──────────────────────────────────────────────────

#[test]
fn test_with_registered_action_metadata() {
    let reg = ActionRegistry::new();
    reg.register_action(ActionMeta {
        name: "validated".into(),
        dcc: "mock".into(),
        input_schema: serde_json::json!({
            "type": "object",
            "required": ["x"],
            "properties": {"x": {"type": "number"}}
        }),
        ..Default::default()
    });
    let d = ActionDispatcher::new(reg);
    d.register_handler("validated", |p| {
        Ok(json!({"doubled": p["x"].as_f64().unwrap_or(0.0) * 2.0}))
    });

    let result = ActionChain::new()
        .step("validated", json!({"x": 5.0}))
        .run(&d, json!({}))
        .unwrap();

    assert!(result.success);
    assert_eq!(result.steps[0].output["doubled"], json!(10.0));
}
