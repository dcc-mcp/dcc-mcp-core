use dcc_mcp_jsonrpc::{
    McpParamDeclaration, McpParamType, McpParamValidationError, build_mcp_param_headers,
    scan_mcp_param_headers, validate_mcp_param_headers,
};
use serde_json::{Value, json};

#[test]
fn nested_headers_keep_exact_property_paths_and_declared_case() {
    let declarations = scan_mcp_param_headers(&json!({
        "type": "object",
        "properties": {
            "routing": {
                "type": "object",
                "properties": {
                    "tenant.name": {"type": "string", "x-mcp-header": "Tenant"}
                }
            }
        }
    }))
    .unwrap();
    assert_eq!(
        declarations,
        vec![McpParamDeclaration {
            path: vec!["routing".into(), "tenant.name".into()],
            header_name: "Tenant".into(),
            value_type: McpParamType::String,
        }]
    );
}

#[test]
fn accepts_only_final_spec_primitives_and_token_suffixes() {
    for (kind, expected) in [
        ("string", McpParamType::String),
        ("integer", McpParamType::Integer),
        ("boolean", McpParamType::Boolean),
    ] {
        let scan = scan_mcp_param_headers(&json!({"properties": {
            "value": {"type": kind, "x-mcp-header": "!#$%&'*+-.^_`|~09Az"}
        }}))
        .unwrap();
        assert_eq!(scan[0].value_type, expected);
    }
    for kind in [
        json!("number"),
        json!("object"),
        json!("array"),
        json!("null"),
        json!(["string", "null"]),
        Value::Null,
    ] {
        assert!(
            scan_mcp_param_headers(&json!({"properties": {
                "value": {"type": kind, "x-mcp-header": "Value"}
            }}))
            .is_err(),
            "type: {kind}"
        );
    }
    for suffix in [
        json!(""),
        json!("has space"),
        json!("a:b"),
        json!("é"),
        json!("a\nb"),
        json!(true),
        Value::Null,
    ] {
        assert!(
            scan_mcp_param_headers(&json!({"properties": {
                "value": {"type": "string", "x-mcp-header": suffix}
            }}))
            .is_err(),
            "suffix: {suffix}"
        );
    }
}

#[test]
fn rejects_case_insensitive_duplicates_and_root_annotations() {
    assert!(
        scan_mcp_param_headers(&json!({"properties": {
            "one": {"type": "string", "x-mcp-header": "Tenant"},
            "two": {"properties": {"nested": {"type": "string", "x-mcp-header": "tenant"}}}
        }}))
        .is_err()
    );
    assert!(scan_mcp_param_headers(&json!({"type": "string", "x-mcp-header": "Root"})).is_err());
}

#[test]
fn rejects_annotations_in_all_non_property_schema_locations() {
    let annotation = json!({"properties": {"deep": {"type": "string", "x-mcp-header": "Hidden"}}});
    for keyword in [
        "items",
        "contains",
        "additionalProperties",
        "unevaluatedProperties",
        "unevaluatedItems",
        "propertyNames",
        "not",
        "if",
        "then",
        "else",
    ] {
        assert!(
            scan_mcp_param_headers(&json!({keyword: annotation})).is_err(),
            "{keyword}"
        );
    }
    for keyword in ["prefixItems", "oneOf", "anyOf", "allOf"] {
        assert!(
            scan_mcp_param_headers(&json!({keyword: [annotation]})).is_err(),
            "{keyword}"
        );
    }
    for keyword in [
        "patternProperties",
        "dependentSchemas",
        "$defs",
        "definitions",
    ] {
        assert!(
            scan_mcp_param_headers(&json!({keyword: {"unused": annotation}})).is_err(),
            "{keyword}"
        );
    }
}

#[test]
fn ordinary_schema_data_is_not_interpreted_as_a_subschema() {
    assert!(
        scan_mcp_param_headers(&json!({
            "examples": [{"x-mcp-header": "literal data"}],
            "const": {"x-mcp-header": "literal data"},
            "properties": {"x-mcp-header": {"type": "string"}}
        }))
        .unwrap()
        .is_empty()
    );
}

#[test]
fn annotations_in_content_and_legacy_subschemas_are_not_silently_lost() {
    let annotation = json!({"properties":{"tenant":{"type":"string", "x-mcp-header":"Tenant"}}});
    for keyword in ["contentSchema", "additionalItems"] {
        assert!(
            scan_mcp_param_headers(&json!({"properties":{"payload": {
                "type":"string", keyword:annotation
            }}}))
            .is_err(),
            "{keyword}"
        );
    }
    assert!(scan_mcp_param_headers(&json!({"dependencies":{"unused":annotation}})).is_err());
}

fn declaration(kind: &str) -> Vec<McpParamDeclaration> {
    scan_mcp_param_headers(&json!({"properties": {
        "value": {"type": kind, "x-mcp-header": "Value"}
    }}))
    .unwrap()
}

#[test]
fn producer_reuses_canonical_encoding_and_skips_absent_or_null() {
    let declarations = declaration("string");
    for (body, header) in [
        ("tenant", "tenant"),
        ("café", "=?base64?Y2Fmw6k=?="),
        ("", "=?base64??="),
        ("d", "d"),
        ("a\tb", "a\tb"),
    ] {
        let arguments = json!({"value": body});
        assert_eq!(
            build_mcp_param_headers(&declarations, &arguments).unwrap(),
            vec![("Mcp-Param-Value".into(), header.into())]
        );
        assert!(
            validate_mcp_param_headers(&declarations, &arguments, |_| Some(header.into())).is_ok()
        );
    }
    for arguments in [json!({}), json!({"value": null})] {
        assert!(
            build_mcp_param_headers(&declarations, &arguments)
                .unwrap()
                .is_empty()
        );
        assert!(
            validate_mcp_param_headers(&declarations, &arguments, |_| Some(
                "valid-but-unneeded".into()
            ))
            .is_ok()
        );
    }
}

#[test]
fn mismatch_missing_and_noncanonical_base64_are_redacted() {
    let declarations = declaration("string");
    for header in [
        None,
        Some("secret-wrong"),
        Some("=?base64?ZE==?="),
        Some("=?base64?/w==?="),
    ] {
        let error = validate_mcp_param_headers(&declarations, &json!({"value": "d"}), |_| {
            header.map(str::to_string)
        })
        .unwrap_err();
        assert!(matches!(
            error,
            McpParamValidationError::HeaderMismatch { .. }
        ));
        assert!(!format!("{error:?}").contains("secret-wrong"));
    }
}

#[test]
fn recognized_malformed_headers_are_rejected_even_when_argument_is_absent() {
    for arguments in [json!({}), json!({"value": null})] {
        for header in [
            "=?base64?ZE==?=",
            "=?base64?/w==?=",
            "raw-é",
            "bad\u{1}value",
        ] {
            assert!(
                matches!(
                    validate_mcp_param_headers(&declaration("string"), &arguments, |_| Some(
                        header.into()
                    )),
                    Err(McpParamValidationError::HeaderMismatch { .. })
                ),
                "{header}"
            );
        }
        assert!(validate_mcp_param_headers(&declaration("string"), &arguments, |_| None).is_ok());
    }
}

#[test]
fn integer_headers_compare_decimal_values_without_float_rounding() {
    let declarations = declaration("integer");
    for body in [json!(42), json!(42.0)] {
        for header in ["42", "42.0", "0042", "00042.000"] {
            assert!(
                validate_mcp_param_headers(&declarations, &json!({"value": body}), |_| Some(
                    header.into()
                ))
                .is_ok(),
                "{header}"
            );
        }
        for header in [
            "42.0000000000000001",
            "+42",
            "4.2e1",
            " 42 ",
            "0x2a",
            "42.",
            "0043",
        ] {
            assert!(
                validate_mcp_param_headers(&declarations, &json!({"value": body}), |_| Some(
                    header.into()
                ))
                .is_err(),
                "{header}"
            );
        }
    }
    for header in ["0", "-0", "-000.000"] {
        assert!(
            validate_mcp_param_headers(&declarations, &json!({"value": 0}), |_| Some(
                header.into()
            ))
            .is_ok()
        );
    }
}

#[test]
fn wrong_types_and_unsafe_integers_are_argument_errors_not_header_errors() {
    for (kind, body) in [
        ("integer", json!(9007199254740992_u64)),
        ("integer", json!(-9007199254740992_i64)),
        ("integer", json!(1.5)),
        ("integer", json!("42")),
        ("boolean", json!("true")),
        ("string", json!({})),
    ] {
        let error =
            validate_mcp_param_headers(&declaration(kind), &json!({"value": body}), |_| None)
                .unwrap_err();
        assert!(matches!(
            error,
            McpParamValidationError::InvalidArgument { .. }
        ));
    }
    for value in [-9007199254740991_i64, 9007199254740991] {
        assert_eq!(
            build_mcp_param_headers(&declaration("integer"), &json!({"value": value})).unwrap(),
            vec![("Mcp-Param-Value".into(), value.to_string())]
        );
    }
    assert_eq!(
        build_mcp_param_headers(&declaration("boolean"), &json!({"value": false})).unwrap(),
        vec![("Mcp-Param-Value".into(), "false".into())]
    );
}
