use dcc_mcp_jsonrpc::{JsonRpcNotification, JsonRpcRequest, JsonRpcResponse, error_codes};
use serde_json::{Value, json};

#[test]
fn request_without_id_deserializes_as_notification() {
    let request: JsonRpcRequest = serde_json::from_value(json!({
        "jsonrpc": "2.0",
        "method": "marketplace.installed.changed"
    }))
    .expect("notification-shaped request should deserialize");

    assert_eq!(request.jsonrpc, "2.0");
    assert_eq!(request.id, None);
    assert_eq!(request.params, None);
}

#[test]
fn response_builders_preserve_json_rpc_wire_shape() {
    let success = serde_json::to_value(JsonRpcResponse::success(
        Some(json!(7)),
        json!({"status": "ok"}),
    ))
    .expect("success response should serialize");
    assert_eq!(
        success,
        json!({"jsonrpc": "2.0", "id": 7, "result": {"status": "ok"}})
    );

    let error = serde_json::to_value(JsonRpcResponse::error_with_data(
        Some(json!(7)),
        error_codes::INVALID_PARAMS,
        "Invalid params: topics array required",
        Some(json!({"field": "topics"})),
    ))
    .expect("error response should serialize");
    assert_eq!(
        error,
        json!({
            "jsonrpc": "2.0",
            "id": 7,
            "error": {
                "code": -32602,
                "message": "Invalid params: topics array required",
                "data": {"field": "topics"}
            }
        })
    );
}

#[test]
fn standard_error_and_notification_builders_are_transport_neutral() {
    let parse_error =
        serde_json::to_value(JsonRpcResponse::parse_error()).expect("parse error should serialize");
    assert_eq!(parse_error["error"]["code"], error_codes::PARSE_ERROR);
    assert_eq!(parse_error["id"], Value::Null);

    let notification = serde_json::to_value(JsonRpcNotification::new(
        "marketplace.operation.completed",
        json!({"operation_id": "op-1"}),
    ))
    .expect("notification should serialize");
    assert_eq!(
        notification,
        json!({
            "jsonrpc": "2.0",
            "method": "marketplace.operation.completed",
            "params": {"operation_id": "op-1"}
        })
    );
    assert!(notification.get("id").is_none());
}
