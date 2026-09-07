//! Focused final-revision response checks; not a full protocol conformance suite.

use dcc_mcp_jsonrpc::{
    CACHEABLE_RESULT_METHODS, DiscoverResult, InitializeResult, SERVER_INFO_META_KEY,
    SUPPORTED_MODERN_PROTOCOL_VERSIONS, ServerCapabilities, ServerDiscoverResult, ServerInfo,
    complete_modern_result,
};
use serde_json::{Value, json};

fn identity() -> ServerInfo {
    ServerInfo {
        name: "dcc-mcp-test".into(),
        version: "1".into(),
    }
}

fn stamp(method: &str, mut result: Value) -> Value {
    complete_modern_result(method, result.as_object_mut().unwrap(), &identity()).unwrap();
    result
}

#[test]
fn modern_versions_match_compiled_support_without_legacy_revisions() {
    #[cfg(feature = "mcp-2026-07-28")]
    assert_eq!(SUPPORTED_MODERN_PROTOCOL_VERSIONS, &["2026-07-28"]);
    #[cfg(not(feature = "mcp-2026-07-28"))]
    assert!(SUPPORTED_MODERN_PROTOCOL_VERSIONS.is_empty());
}

#[test]
fn official_discovery_fixture_uses_the_single_canonical_type() {
    // Official SDK commit 5119ee7fd7790e335a3fb60ef36f85334e2a6326:
    // packages/core-internal/test/corpus/fixtures/2026-07-28/
    // DiscoverResultResponse/discover-result-response.json
    let fixture: Value =
        serde_json::from_str(include_str!("fixtures/modern_discover_response.json")).unwrap();
    let result: DiscoverResult = serde_json::from_value(fixture["result"].clone()).unwrap();
    let alias: ServerDiscoverResult = result;
    let encoded = serde_json::to_value(alias).unwrap();
    for field in [
        "supportedVersions",
        "resultType",
        "ttlMs",
        "cacheScope",
        "_meta",
    ] {
        assert_eq!(encoded[field], fixture["result"][field], "{field}");
    }
    assert!(encoded.get("serverInfo").is_none());
    assert!(encoded.get("protocolVersion").is_none());
    for required in ["supportedVersions", "resultType", "ttlMs", "cacheScope"] {
        let mut invalid = fixture["result"].clone();
        invalid.as_object_mut().unwrap().remove(required);
        assert!(
            serde_json::from_value::<DiscoverResult>(invalid).is_err(),
            "{required}"
        );
    }
}

#[test]
fn modern_cache_fill_is_a_closed_method_set() {
    assert_eq!(CACHEABLE_RESULT_METHODS.len(), 6);
    for method in [
        "server/discover",
        "tools/list",
        "prompts/list",
        "resources/list",
        "resources/templates/list",
        "resources/read",
    ] {
        let result = stamp(method, json!({}));
        assert_eq!(result["resultType"], "complete", "{method}");
        assert_eq!(result["ttlMs"], 0, "{method}");
        assert_eq!(result["cacheScope"], "private", "{method}");
        assert_eq!(
            result["_meta"][SERVER_INFO_META_KEY]["name"],
            "dcc-mcp-test"
        );
    }
    for method in [
        "tools/call",
        "prompts/get",
        "ping",
        "completion/complete",
        "custom/read",
    ] {
        let result = stamp(method, json!({}));
        assert_eq!(result["resultType"], "complete");
        assert!(result.get("ttlMs").is_none(), "{method}");
        assert!(result.get("cacheScope").is_none(), "{method}");
    }
}

#[test]
fn business_payload_and_authored_metadata_are_preserved() {
    for is_error in [false, true] {
        let original = json!({
            "content": [{"type": "text", "text": "readback"}],
            "structuredContent": {"host": "blender", "value": 3},
            "isError": is_error,
            "_meta": {"dcc.next_tools": ["inspect"], SERVER_INFO_META_KEY: {"name": "adapter", "version": "2", "title": "Adapter identity"}}
        });
        let stamped = stamp("tools/call", original.clone());
        for field in ["content", "structuredContent", "isError", "_meta"] {
            assert_eq!(stamped[field], original[field]);
        }
        assert_eq!(stamped["resultType"], "complete");
    }
}

#[test]
fn malformed_authored_identity_uses_configured_identity_without_losing_payload() {
    for invalid in [
        json!(null),
        json!(false),
        json!("adapter"),
        json!([]),
        json!({}),
        json!({"name": "adapter"}),
        json!({"name": 3, "version": "2"}),
    ] {
        let result = stamp(
            "tools/call",
            json!({
                "content": [{"type": "text", "text": "completed"}],
                "isError": false,
                "_meta": {"dcc.readback": true, SERVER_INFO_META_KEY: invalid}
            }),
        );
        assert_eq!(result["_meta"][SERVER_INFO_META_KEY], json!(identity()));
        assert_eq!(result["_meta"]["dcc.readback"], true);
        assert_eq!(result["content"][0]["text"], "completed");
        assert_eq!(result["isError"], false);
        assert_eq!(result["resultType"], "complete");
    }
}

#[test]
fn cache_hints_preserve_valid_values_and_default_invalid_values() {
    let hinted = stamp(
        "resources/read",
        json!({"ttlMs": 125, "cacheScope": "public"}),
    );
    assert_eq!(hinted["ttlMs"], 125);
    assert_eq!(hinted["cacheScope"], "public");
    for invalid in [
        json!(-1),
        json!(1.5),
        json!("1000"),
        json!(null),
        json!(true),
        json!(9007199254740992_u64),
    ] {
        let stamped = stamp(
            "resources/read",
            json!({"ttlMs": invalid, "cacheScope": "shared"}),
        );
        assert_eq!(stamped["ttlMs"], 0);
        assert_eq!(stamped["cacheScope"], "private");
    }
}

#[test]
fn unsupported_result_kinds_and_malformed_metadata_fail_before_changes() {
    for mut result in [
        json!({"resultType": "input_required", "requestState": "opaque"}),
        json!({"_meta": false}),
    ] {
        let before = result.clone();
        assert!(
            complete_modern_result("tools/call", result.as_object_mut().unwrap(), &identity())
                .is_err()
        );
        assert_eq!(result, before);
    }
}

#[test]
fn legacy_initialize_serialization_has_no_modern_result_fields() {
    let result = InitializeResult {
        protocol_version: "2025-06-18".into(),
        capabilities: ServerCapabilities::default(),
        server_info: identity(),
        instructions: None,
    };
    assert_eq!(
        serde_json::to_value(result).unwrap(),
        json!({
            "protocolVersion": "2025-06-18", "capabilities": {},
            "serverInfo": {"name": "dcc-mcp-test", "version": "1"}
        })
    );
}
