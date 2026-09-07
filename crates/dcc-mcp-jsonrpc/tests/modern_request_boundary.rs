//! Bounded final-revision ingress checks, not full protocol conformance.

use dcc_mcp_jsonrpc::{
    CLIENT_CAPABILITIES_META_KEY as CAPS, CLIENT_INFO_META_KEY as INFO, InboundRoute,
    JsonRpcRequestBuilder, JsonRpcResponse, LOG_LEVEL_META_KEY as LOG,
    PROTOCOL_VERSION_META_KEY as VERSION, ProtocolRequestHints, StatelessRequestMeta,
    classify_protocol_request, decode_mcp_header_value, encode_mcp_header_value,
    strip_request_envelope,
};
use serde_json::{Value, json};

const MODERN: &str = "2026-07-28";

fn body(method: &str, params: Value) -> Value {
    let meta = StatelessRequestMeta::parse(Some(&json!({VERSION: MODERN, CAPS: {}}))).unwrap();
    JsonRpcRequestBuilder::new("boundary", method)
        .with_params(params)
        .with_stateless_metadata(&meta)
        .unwrap()
        .to_value()
}

fn classify(
    body: &Value,
    version: Option<&str>,
    method: Option<&str>,
    name: Option<&str>,
) -> Result<InboundRoute, JsonRpcResponse> {
    classify_protocol_request(
        "POST",
        ProtocolRequestHints {
            protocol_version: version,
            method,
            name,
            ..Default::default()
        },
        body,
        &[MODERN],
    )
}

fn error_code(result: Result<InboundRoute, JsonRpcResponse>) -> i64 {
    result.unwrap_err().error.unwrap().code
}

#[test]
fn pinned_negative_fixture_matches_the_shared_classifier() {
    let cases: Vec<Value> =
        serde_json::from_str(include_str!("fixtures/modern_request_cases.json")).unwrap();
    assert_eq!(cases.len(), 18);
    for case in cases {
        let h = &case["headers"];
        let result = classify(
            &case["body"],
            h["MCP-Protocol-Version"].as_str(),
            h["Mcp-Method"].as_str(),
            h["Mcp-Name"].as_str(),
        );
        if case["route"] == "legacy" {
            assert!(matches!(result, Ok(InboundRoute::Legacy)), "{}", case["name"]);
            continue;
        }
        if case["route"] == "modern" {
            assert!(matches!(result, Ok(InboundRoute::Modern(_))), "{}", case["name"]);
            continue;
        }
        assert_eq!(
            error_code(result),
            case["code"].as_i64().unwrap(),
            "{}",
            case["name"]
        );
    }
}

#[test]
fn body_claim_never_silently_upgrades_or_downgrades() {
    let request = body("tools/list", json!({}));
    assert!(matches!(
        classify(&request, Some(MODERN), Some("tools/list"), None),
        Ok(InboundRoute::Modern(_))
    ));
    assert_eq!(
        error_code(classify(&request, None, Some("tools/list"), None)),
        -32020
    );
    assert_eq!(
        error_code(classify(
            &request,
            Some("2025-06-18"),
            Some("tools/list"),
            None
        )),
        -32020
    );
    let bare = JsonRpcRequestBuilder::new(1, "tools/list")
        .with_params(json!({}))
        .to_value();
    assert_eq!(
        error_code(classify(&bare, Some(MODERN), Some("tools/list"), None)),
        -32602
    );
    let rc = json!({"jsonrpc":"2.0","id":1,"method":"tools/list","params":{"_meta":{"protocolVersion":MODERN,"clientCapabilities":{}}}});
    assert_eq!(
        error_code(classify(&rc, Some(MODERN), Some("tools/list"), None)),
        -32602
    );
}

#[test]
fn envelope_requiredness_and_present_field_types_are_validated() {
    for (key, invalid) in [
        (VERSION, json!(null)),
        (VERSION, json!(42)),
        (CAPS, json!(null)),
        (CAPS, json!([])),
        (CAPS, json!({"roots":{"listChanged":"yes"}})),
        (CAPS, json!({"sampling":{"context":false}})),
        (CAPS, json!({"elicitation":{"form":{"applyDefaults":42}}})),
        (CAPS, json!({"extensions":{"vendor":false}})),
        (INFO, json!(null)),
        (INFO, json!({"name":"test"})),
        (
            INFO,
            json!({"name":"test","version":"1","icons":[{"src":4}]}),
        ),
        ("progressToken", json!(0.5)),
        ("progressToken", json!(true)),
        ("progressToken", json!(9_007_199_254_740_992_u64)),
        (LOG, json!("verbose")),
    ] {
        let mut request = body("tools/list", json!({}));
        request["params"]["_meta"][key] = invalid;
        assert_eq!(
            error_code(classify(&request, Some(MODERN), Some("tools/list"), None)),
            -32602,
            "{request}"
        );
    }
    for missing in [VERSION, CAPS] {
        let mut request = body("tools/list", json!({}));
        request["params"]["_meta"]
            .as_object_mut()
            .unwrap()
            .remove(missing);
        assert_eq!(
            error_code(classify(&request, Some(MODERN), Some("tools/list"), None)),
            -32602
        );
    }
}

#[test]
fn optional_identity_and_foreign_metadata_roundtrip_without_business_leaks() {
    let original = json!({VERSION:MODERN, CAPS:{}, "progressToken":3.0,
        "traceparent":"trace", "vendor/context":{"x":1},
        INFO:{"name":"test","version":"1","title":"Display","icons":[{"src":"icon.svg"}]}, LOG:"info"});
    let meta: StatelessRequestMeta = serde_json::from_value(original.clone()).unwrap();
    assert_eq!(serde_json::to_value(&meta).unwrap(), original);
    assert!(
        StatelessRequestMeta::parse(Some(&json!({VERSION: MODERN, CAPS: {}})))
            .unwrap()
            .client_info
            .is_none()
    );
    let mut request = JsonRpcRequestBuilder::new(1, "tools/call")
        .with_params(json!({"name":"test","arguments":{"_meta":{VERSION:"business"}},"_meta":{"vendor/existing":true}}))
        .with_stateless_metadata(&meta).unwrap().to_value();
    strip_request_envelope(&mut request["params"]);
    let remaining = &request["params"]["_meta"];
    for key in [VERSION, CAPS, INFO, LOG] {
        assert!(remaining.get(key).is_none());
    }
    assert_eq!(remaining["vendor/existing"], true);
    assert_eq!(remaining["progressToken"], 3.0);
    assert_eq!(remaining["traceparent"], "trace");
    assert_eq!(request["params"]["arguments"]["_meta"][VERSION], "business");
}

#[test]
fn unsupported_version_error_uses_final_code_and_data_names() {
    let mut request = body("tools/list", json!({}));
    request["params"]["_meta"][VERSION] = json!("2099-01-01");
    let response = classify(&request, Some("2099-01-01"), Some("tools/list"), None).unwrap_err();
    assert_eq!(response.id, Some(json!("boundary")));
    let error = response.error.unwrap();
    assert_eq!(error.code, -32022);
    assert_eq!(
        error.data.unwrap(),
        json!({"supported":[MODERN],"requested":"2099-01-01"})
    );
    let required =
        JsonRpcResponse::missing_required_client_capability(Some(json!(1)), json!({"sampling":{}}));
    assert_eq!(required.error.as_ref().unwrap().code, -32021);
    assert_eq!(
        required.error.unwrap().data.unwrap(),
        json!({"requiredCapabilities":{"sampling":{}}})
    );
}

#[test]
fn standard_headers_validate_presence_mismatch_and_canonical_unicode() {
    let request = body("tools/call", json!({"name":"Hello, 世界","arguments":{}}));
    let encoded = encode_mcp_header_value("Hello, 世界");
    assert!(matches!(
        classify(&request, Some(MODERN), Some("tools/call"), Some(&encoded)),
        Ok(InboundRoute::Modern(_))
    ));
    for (method, name) in [
        (None, Some(encoded.as_str())),
        (Some("tools/list"), Some(encoded.as_str())),
        (Some("tools/call"), None),
        (Some("tools/call"), Some("other")),
        (Some("tools/call"), Some("=?base64?SGVs!!!bG8=?=")),
    ] {
        assert_eq!(
            error_code(classify(&request, Some(MODERN), method, name)),
            -32020
        );
    }
    let plain = body("resources/read", json!({"uri":"file:///a"}));
    assert!(matches!(
        classify(
            &plain,
            Some(" 2026-07-28\t"),
            Some("\tresources/read "),
            Some(" file:///a\t")
        ),
        Ok(InboundRoute::Modern(_))
    ));
    assert_eq!(
        error_code(classify(&plain, Some(MODERN), Some("resources/read"), None)),
        -32020
    );
    let absent_name = body("tools/call", json!({}));
    assert!(matches!(
        classify(&absent_name, Some(MODERN), Some("tools/call"), None),
        Ok(InboundRoute::Modern(_))
    ));
    for value in [
        "",
        " echo ",
        "Hello, 世界",
        "=?base64?YQ==?=",
        "line\nbreak",
    ] {
        assert_eq!(
            decode_mcp_header_value(&encode_mcp_header_value(value)).as_deref(),
            Some(value)
        );
    }
    for invalid in ["=?base64?YQ?=", "=?base64?YR==?=", "=?base64?/w==?="] {
        assert!(decode_mcp_header_value(invalid).is_none());
    }
}

#[test]
fn legacy_and_notification_paths_retain_their_own_contracts() {
    for request in [
        json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{"protocolVersion":"2025-06-18"}}),
        json!({"jsonrpc":"2.0","id":2,"method":"tools/call","params":{"name":"test","_meta":{INFO:{"name":"vendor"}}}}),
        json!([{"jsonrpc":"2.0","id":3,"method":"tools/list"}]),
    ] {
        let before = request.clone();
        assert!(matches!(
            classify(&request, Some("2025-06-18"), None, None),
            Ok(InboundRoute::Legacy)
        ));
        assert_eq!(request, before);
    }
    let mut notification = body("notifications/custom", json!({}));
    notification.as_object_mut().unwrap().remove("id");
    notification["params"]["_meta"]
        .as_object_mut()
        .unwrap()
        .remove(CAPS);
    assert!(matches!(
        classify(&notification, None, None, None),
        Ok(InboundRoute::Modern(_))
    ));
    assert_eq!(
        error_code(classify(
            &json!([body("tools/list", json!({}))]),
            None,
            None,
            None
        )),
        -32600
    );
    for id in [json!(null), json!(false), json!(0.5)] {
        let mut request = body("tools/list", json!({}));
        request["id"] = id;
        assert_eq!(
            error_code(classify(&request, Some(MODERN), Some("tools/list"), None)),
            -32600
        );
    }
}
