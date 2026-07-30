use super::*;
use serde_json::json;

#[test]
fn output_format_parsing() {
    assert_eq!(
        OutputFormat::from_flag("human").unwrap(),
        OutputFormat::Human
    );
    assert_eq!(
        OutputFormat::from_flag("pretty").unwrap(),
        OutputFormat::Human
    );
    assert_eq!(OutputFormat::from_flag("json").unwrap(), OutputFormat::Json);
    assert_eq!(
        OutputFormat::from_flag("ndjson").unwrap(),
        OutputFormat::Ndjson
    );
    assert_eq!(OutputFormat::from_flag("toon").unwrap(), OutputFormat::Toon);
    assert!(OutputFormat::from_flag("xml").is_err());
    assert!(OutputFormat::from_flag("").is_err());
}

#[test]
fn output_format_case_insensitive() {
    assert_eq!(OutputFormat::from_flag("JSON").unwrap(), OutputFormat::Json);
    assert_eq!(
        OutputFormat::from_flag("NdJson").unwrap(),
        OutputFormat::Ndjson
    );
    assert_eq!(
        OutputFormat::from_flag("HUMAN").unwrap(),
        OutputFormat::Human
    );
    assert_eq!(OutputFormat::from_flag("TOON").unwrap(), OutputFormat::Toon);
}

#[test]
fn toon_output_round_trips() {
    let expected = json!({
        "total": 2,
        "instances": [
            {"dcc_type": "unreal", "scene": "DemoMap"},
            {"dcc_type": "maya", "scene": "shot_010"},
        ],
    });

    let encoded = serialize_value(OutputFormat::Toon, &expected).unwrap();
    let decoded: serde_json::Value = toon_format::decode_default(&encoded).unwrap();

    assert_eq!(decoded, expected);
}

#[test]
fn exit_code_values() {
    assert_eq!(ExitCode::Success.as_i32(), 0);
    assert_eq!(ExitCode::GeneralError.as_i32(), 1);
    assert_eq!(ExitCode::InvalidInput.as_i32(), 2);
    assert_eq!(ExitCode::Unavailable.as_i32(), 3);
    assert_eq!(ExitCode::Timeout.as_i32(), 4);
    assert_eq!(ExitCode::Cancelled.as_i32(), 5);
    assert_eq!(ExitCode::PermissionDenied.as_i32(), 6);
    assert_eq!(ExitCode::Conflict.as_i32(), 7);
}

#[test]
fn exit_code_from_http_status() {
    assert_eq!(ExitCode::from_http_status(200), ExitCode::Success);
    assert_eq!(ExitCode::from_http_status(201), ExitCode::Success);
    assert_eq!(ExitCode::from_http_status(400), ExitCode::InvalidInput);
    assert_eq!(ExitCode::from_http_status(401), ExitCode::PermissionDenied);
    assert_eq!(ExitCode::from_http_status(403), ExitCode::PermissionDenied);
    assert_eq!(ExitCode::from_http_status(404), ExitCode::InvalidInput);
    assert_eq!(ExitCode::from_http_status(408), ExitCode::Timeout);
    assert_eq!(ExitCode::from_http_status(409), ExitCode::Conflict);
    assert_eq!(ExitCode::from_http_status(429), ExitCode::Unavailable);
    assert_eq!(ExitCode::from_http_status(500), ExitCode::Unavailable);
    assert_eq!(ExitCode::from_http_status(503), ExitCode::Unavailable);
    assert_eq!(ExitCode::from_http_status(418), ExitCode::GeneralError);
}

#[test]
fn error_envelope_serialization() {
    let env = ErrorEnvelope::new(
        "TIMEOUT",
        "operation timed out after 30s",
        ExitCode::Timeout,
    )
    .with_retryable(true);
    let json = serde_json::to_string(&env).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["error"]["code"], "TIMEOUT");
    assert_eq!(parsed["error"]["message"], "operation timed out after 30s");
    assert_eq!(parsed["error"]["exit_code"], 4);
    assert_eq!(parsed["error"]["retryable"], true);
    assert!(parsed["error"]["details"].is_null());
}

#[test]
fn error_envelope_with_details() {
    let env = ErrorEnvelope::new(
        "INVALID_INPUT",
        "missing required field",
        ExitCode::InvalidInput,
    )
    .with_details(json!({"field": "dcc_type", "command": "call"}));
    let json = serde_json::to_string(&env).unwrap();
    let parsed: serde_json::Value = serde_json::from_str(&json).unwrap();
    assert_eq!(parsed["error"]["details"]["field"], "dcc_type");
}

#[test]
fn is_list_payload_detection() {
    let list = json!({"instances": [], "gateway": {"current": null}});
    assert!(is_list_payload(&list));
    let not_list = json!({"ok": true});
    assert!(!is_list_payload(&not_list));
}
