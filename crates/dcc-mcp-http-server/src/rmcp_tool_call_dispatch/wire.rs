//! Executor wire encode/decode for cross-thread tool dispatch.

use serde_json::{Value, json};

use dcc_mcp_actions::{DispatchError, DispatchResult};
use dcc_mcp_models::ThreadAffinity;

pub(crate) fn use_main_thread_route(
    thread_affinity: ThreadAffinity,
    executor_present: bool,
) -> bool {
    matches!(thread_affinity, ThreadAffinity::Main) && executor_present
}

pub(crate) fn encode_dispatch_wire(result: Result<DispatchResult, DispatchError>) -> String {
    match result {
        Ok(r) => serde_json::to_string(&json!({
            "__dispatch_ok": {
                "action": r.action,
                "output": r.output,
                "validation_skipped": r.validation_skipped,
            }
        }))
        .unwrap_or_else(|_| "{\"__dispatch_ok\":{}}".to_string()),
        Err(err) => encode_dispatch_error_wire(&err),
    }
}

fn encode_dispatch_error_wire(err: &DispatchError) -> String {
    let payload = match err {
        DispatchError::HandlerNotFound(n) => json!({
            "__dispatch_error_kind": "handler_not_found",
            "message": n,
        }),
        DispatchError::MetadataNotFound(n) => json!({
            "__dispatch_error_kind": "metadata_not_found",
            "message": n,
        }),
        DispatchError::ValidationFailed(m) => json!({
            "__dispatch_error_kind": "validation_failed",
            "message": m,
        }),
        DispatchError::HandlerError(m) => json!({
            "__dispatch_error_kind": "handler_error",
            "message": m,
        }),
        DispatchError::ActionDisabled { action, group } => json!({
            "__dispatch_error_kind": "action_disabled",
            "action": action,
            "group": group,
        }),
        DispatchError::ThreadAffinityViolation {
            action,
            declared,
            actual,
        } => json!({
            "__dispatch_error_kind": "thread_affinity_violation",
            "action": action,
            "declared": declared.to_string(),
            "actual": actual.to_string(),
        }),
    };
    serde_json::to_string(&payload).unwrap_or_else(|_| {
        "{\"__dispatch_error_kind\":\"handler_error\",\"message\":\"dispatch failure\"}".to_string()
    })
}

pub(crate) fn decode_dispatch_wire(json_str: &str) -> Result<DispatchResult, DispatchError> {
    let value: Value = serde_json::from_str(json_str).unwrap_or(json!({}));
    if let Some(ok) = value.get("__dispatch_ok") {
        let action = ok
            .get("action")
            .and_then(Value::as_str)
            .unwrap_or("")
            .to_string();
        let output = ok.get("output").cloned().unwrap_or(Value::Null);
        let validation_skipped = ok
            .get("validation_skipped")
            .and_then(Value::as_bool)
            .unwrap_or(false);
        return Ok(DispatchResult {
            action,
            output,
            validation_skipped,
        });
    }
    if value.get("__dispatch_error_kind").is_some() {
        return Err(decode_dispatch_error_payload(&value));
    }
    if let Some(err) = value.get("__dispatch_error").and_then(Value::as_str) {
        return Err(DispatchError::HandlerError(err.to_string()));
    }
    Err(DispatchError::HandlerError(
        "malformed dispatch wire payload".to_string(),
    ))
}

fn decode_dispatch_error_payload(value: &Value) -> DispatchError {
    let kind = value
        .get("__dispatch_error_kind")
        .and_then(Value::as_str)
        .unwrap_or("handler_error");
    let message = value
        .get("message")
        .and_then(Value::as_str)
        .unwrap_or("dispatch error")
        .to_string();
    match kind {
        "handler_not_found" => DispatchError::HandlerNotFound(message),
        "metadata_not_found" => DispatchError::MetadataNotFound(message),
        "validation_failed" => DispatchError::ValidationFailed(message),
        "handler_error" => DispatchError::HandlerError(message),
        "action_disabled" => DispatchError::ActionDisabled {
            action: value
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
            group: value
                .get("group")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string(),
        },
        "thread_affinity_violation" => {
            let action = value
                .get("action")
                .and_then(Value::as_str)
                .unwrap_or("unknown")
                .to_string();
            let declared = value
                .get("declared")
                .and_then(Value::as_str)
                .and_then(dcc_mcp_models::ThreadAffinity::parse)
                .unwrap_or(ThreadAffinity::Main);
            let actual = value
                .get("actual")
                .and_then(Value::as_str)
                .and_then(dcc_mcp_models::ThreadAffinity::parse)
                .unwrap_or(ThreadAffinity::Any);
            DispatchError::ThreadAffinityViolation {
                action,
                declared,
                actual,
            }
        }
        _ => DispatchError::HandlerError(message),
    }
}

/// MCP hot path — callers only need the handler output [`Value`].
pub(crate) fn decode_dispatch_output(json_str: &str) -> Result<Value, String> {
    decode_dispatch_wire(json_str)
        .map(|r| r.output)
        .map_err(|e| e.to_string())
}
