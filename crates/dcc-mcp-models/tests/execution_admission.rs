use dcc_mcp_models::{ExecutionMode, is_valid_progress_token};
use serde_json::json;

#[test]
fn absent_or_invalid_progress_does_not_request_a_job() {
    assert!(!ExecutionMode::Sync.should_dispatch_async(false, None));
    for token in [json!(null), json!(false), json!(true), json!([]), json!({})] {
        assert!(!is_valid_progress_token(&token), "{token}");
        assert!(!ExecutionMode::Sync.should_dispatch_async(false, Some(&token)));
        assert!(ExecutionMode::Async.should_dispatch_async(false, Some(&token)));
        assert!(ExecutionMode::Sync.should_dispatch_async(true, Some(&token)));
    }
}

#[test]
fn valid_progress_or_explicit_execution_requests_a_job() {
    assert!(ExecutionMode::Async.should_dispatch_async(false, None));
    assert!(ExecutionMode::Sync.should_dispatch_async(true, None));
    for token in [
        json!(""),
        json!("progress"),
        json!(0),
        json!(-1),
        json!(0.5),
    ] {
        assert!(is_valid_progress_token(&token), "{token}");
        assert!(ExecutionMode::Sync.should_dispatch_async(false, Some(&token)));
    }
}
