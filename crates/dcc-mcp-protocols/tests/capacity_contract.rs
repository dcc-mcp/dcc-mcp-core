//! Conformance tests for the shared bridge capacity contract (issue #2821).

use dcc_mcp_protocols::capacity_contract as contract;
use serde_json::{Value, json};
use std::fs;
use std::path::PathBuf;

fn vectors() -> Value {
    let path = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../../contracts/bridge-capacity-v1.conformance.json");
    serde_json::from_str(&fs::read_to_string(path).expect("conformance vectors"))
        .expect("valid JSON")
}

fn vector(id: &str) -> Value {
    vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["id"] == id)
        .cloned()
        .unwrap_or_else(|| panic!("missing vector {}", id))
}

fn capabilities(vector: &Value) -> serde_json::Map<String, Value> {
    vector["capabilities"]
        .as_object()
        .cloned()
        .expect("capabilities object")
}

fn policy(vector: &Value) -> contract::CapacityPolicy {
    if vector.get("policy").is_none() {
        return contract::CapacityPolicy::default();
    }
    contract::CapacityPolicy::new(
        vector["policy"]["min_bytes"].as_u64().unwrap(),
        vector["policy"]["max_bytes"].as_u64().unwrap(),
    )
    .unwrap()
}

#[test]
fn exact_limit_and_limit_plus_one_use_final_utf8_bytes() {
    let exact = vector("exact_limit_ascii");
    let over = vector("limit_plus_one_ascii");
    let exact_budget = contract::validate_final_text(
        exact["payload"].as_str().unwrap(),
        exact["limit_bytes"].as_u64().unwrap(),
    )
    .unwrap();
    let over_budget = contract::validate_final_text(
        over["payload"].as_str().unwrap(),
        over["limit_bytes"].as_u64().unwrap(),
    )
    .unwrap();

    assert_eq!(
        exact_budget.actual_bytes,
        exact["expected_bytes"].as_u64().unwrap() as usize
    );
    assert!(exact_budget.fits);
    assert_eq!(
        over_budget.actual_bytes,
        over["expected_bytes"].as_u64().unwrap() as usize
    );
    assert!(!over_budget.fits);
}

#[test]
fn cjk_emoji_and_json_escapes_count_serialized_bytes() {
    let cjk = vector("cjk_and_emoji");
    let escapes = vector("json_escapes");

    let cjk_budget = contract::validate_final_text(cjk["payload"].as_str().unwrap(), 20).unwrap();
    let escape_budget =
        contract::validate_final_text(escapes["payload"].as_str().unwrap(), 16).unwrap();

    assert_eq!(
        cjk_budget.actual_bytes,
        cjk["expected_bytes"].as_u64().unwrap() as usize
    );
    assert_eq!(
        escape_budget.actual_bytes,
        escapes["expected_bytes"].as_u64().unwrap() as usize
    );
}

#[test]
fn long_id_envelope_overhead_counts_all_final_bytes() {
    let vector = vector("long_request_id_envelope_overhead");
    let raw = serde_json::to_vec(&vector["message"]).unwrap();
    let text = String::from_utf8(raw.clone()).unwrap();

    assert!(text.contains(vector["message"]["id"].as_str().unwrap()));
    assert!(raw.len() >= vector["min_bytes"].as_u64().unwrap() as usize);

    let budget =
        contract::validate_final_bytes(&raw, vector["limit_bytes"].as_u64().unwrap()).unwrap();
    assert!(budget.fits);
    assert_eq!(budget.actual_bytes, raw.len());

    let smaller = contract::validate_final_bytes(&raw, 64).unwrap();
    assert!(!smaller.fits);
    assert_eq!(smaller.actual_bytes, raw.len());
}

#[test]
fn already_serialized_bytes_are_not_re_encoded() {
    let raw = serde_json::to_vec(&json!({"id": "模型🙂", "quote": "a\"b"})).unwrap();
    let budget = contract::validate_final_bytes(&raw, raw.len() as u64).unwrap();

    assert_eq!(budget.actual_bytes, raw.len());
    assert!(budget.fits);
}

#[test]
fn directional_declaration_legacy_and_policy_clamping() {
    for id in &[
        "legacy_flat_declaration",
        "asymmetric_directional_declaration",
        "only_one_direction_declared",
        "huge_declaration_is_clamped",
        "small_declaration_is_clamped_up",
    ] {
        let item = vector(id);
        let state = contract::parse_capacity_declaration(Some(&capabilities(&item)), policy(&item))
            .unwrap();
        assert_eq!(state.negotiation.as_str(), contract::DECLARED);
        assert_eq!(
            state.receive.incoming,
            item["expected_receive"]["incoming"].as_u64().unwrap()
        );
        assert_eq!(
            state.receive.outgoing,
            item["expected_receive"]["outgoing"].as_u64().unwrap()
        );
    }

    for id in &["missing_declaration", "legacy_alternate_key"] {
        let item = vector(id);
        let state = contract::parse_capacity_declaration(Some(&capabilities(&item)), policy(&item))
            .unwrap();
        assert_eq!(state.negotiation.as_str(), contract::LEGACY_ASSUMED);
        assert_eq!(
            state.receive.incoming,
            item["expected_receive"]["incoming"].as_u64().unwrap()
        );
        assert_eq!(
            state.receive.outgoing,
            item["expected_receive"]["outgoing"].as_u64().unwrap()
        );
    }

    let none =
        contract::parse_capacity_declaration(None, contract::CapacityPolicy::default()).unwrap();
    assert_eq!(
        none.receive,
        contract::DirectionalReceive::new(65535, 65535).unwrap()
    );
}

#[test]
fn capacity_policy_rejects_zero_and_inverted_bounds() {
    assert!(contract::CapacityPolicy::new(0, 256).is_err());
    assert!(contract::CapacityPolicy::new(512, 256).is_err());
    let policy = contract::CapacityPolicy::new(256, 512).unwrap();
    assert_eq!(policy.min_bytes(), 256);
    assert_eq!(policy.max_bytes(), 512);
}

#[test]
fn invalid_declarations_are_rejected_deterministically() {
    let invalid: Vec<Value> = vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "invalid_capability")
        .cloned()
        .collect();
    assert!(invalid.len() >= 6);

    for item in invalid {
        let error = contract::parse_capacity_declaration(Some(&capabilities(&item)), policy(&item))
            .expect_err("declaration must be rejected");
        assert_eq!(error.code, item["expected_error"].as_str().unwrap());
    }
}

#[test]
fn u64_boundary_declarations_use_shared_vectors() {
    let domain = &vectors()["numeric_domain"];
    let at_max = vector("u64_max_declaration_is_clamped");
    let overflow = vector("u64_overflow_declaration_is_invalid");

    assert_eq!(domain["receive_capacity"], "u64");
    assert_eq!(domain["max"].as_u64().unwrap(), contract::MAX_U64);
    assert!(
        at_max["capabilities"]["dcc-mcp.capacity.v1"]["receive"]["incoming"]
            .as_u64()
            .is_some()
    );

    let state = contract::parse_capacity_declaration(Some(&capabilities(&at_max)), policy(&at_max))
        .unwrap();
    assert_eq!(state.receive.to_json(), at_max["expected_receive"]);

    let error =
        contract::parse_capacity_declaration(Some(&capabilities(&overflow)), policy(&overflow))
            .expect_err("value above u64::MAX must be rejected");
    assert_eq!(error.code, overflow["expected_error"].as_str().unwrap());
}

#[test]
fn invalid_error_data_vectors_are_rejected() {
    let invalid: Vec<Value> = vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "invalid_error_data")
        .cloned()
        .collect();
    assert!(invalid.len() >= 2);

    for item in invalid {
        let mut data = json!({
            "direction": item["direction"],
            "max_bytes": item["max_bytes"],
            "outcome": item["outcome"],
        });
        if let Some(actual) = item.get("actual_bytes") {
            data["actual_bytes"] = actual.clone();
        }
        let error = contract::parse_capacity_error_data(&data).expect_err("invalid error data");
        assert_eq!(error.code, item["expected_error"].as_str().unwrap());
    }
}

#[test]
fn u64_boundary_error_data_uses_shared_vectors() {
    let at_max = vector("error_data_u64_max_values");
    let overflow = vector("error_data_u64_overflow_is_invalid");

    let data = contract::capacity_error_data(
        at_max["direction"].as_str().unwrap(),
        at_max["max_bytes"].as_u64().unwrap(),
        at_max["outcome"].as_str().unwrap(),
        at_max["actual_bytes"].as_u64(),
    )
    .unwrap();
    let wire = serde_json::to_vec(&data).unwrap();
    let parsed =
        contract::parse_capacity_error_data(&serde_json::from_slice(&wire).unwrap()).unwrap();
    assert_eq!(parsed, data);
    assert_eq!(parsed["max_bytes"].as_u64().unwrap(), contract::MAX_U64);
    assert_eq!(parsed["actual_bytes"].as_u64().unwrap(), contract::MAX_U64);

    let mut invalid = overflow.clone();
    invalid.as_object_mut().unwrap().remove("expected_error");
    let error = contract::parse_capacity_error_data(&invalid).expect_err("value above u64::MAX");
    assert_eq!(error.code, overflow["expected_error"].as_str().unwrap());
}

#[test]
fn structured_error_data_round_trips_without_payload_echo() {
    let items: Vec<Value> = vectors()["vectors"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item["kind"] == "error_data")
        .cloned()
        .collect();

    for item in items {
        let actual = item
            .get("actual_bytes")
            .map(|value| value.as_u64().unwrap());
        let data = contract::capacity_error_data(
            item["direction"].as_str().unwrap(),
            item["max_bytes"].as_u64().unwrap(),
            item["outcome"].as_str().unwrap(),
            actual,
        )
        .unwrap();
        let wire = serde_json::to_vec(&data).unwrap();
        let decoded: Value = serde_json::from_slice(&wire).unwrap();
        let parsed = contract::parse_capacity_error_data(&decoded).unwrap();

        assert_eq!(parsed, data);
        assert_eq!(parsed["outcome"], item["outcome"]);
        assert_eq!(parsed.get("actual_bytes"), item.get("actual_bytes"));
        assert!(parsed.get("content").is_none());
    }
}

#[test]
fn error_budget_domain_requires_positive_integer_and_final_byte_comparison() {
    let domain = &vectors()["error_budget_domain"];
    let budget = vectors()["error_budget_bytes"].as_u64().unwrap();
    let item = vector("error_data_unknown_without_size");

    assert_eq!(domain["type"], "positive_integer");
    assert_eq!(domain["min"].as_u64().unwrap(), 1);
    assert_eq!(
        domain["comparison"],
        "final_serialized_utf8_byte_length <= error_budget_bytes"
    );
    assert!(budget >= domain["min"].as_u64().unwrap());

    let payload = contract::make_compact_capacity_error(
        item["direction"].as_str().unwrap(),
        item["max_bytes"].as_u64().unwrap(),
        budget,
        "correlation-id",
        item["outcome"].as_str().unwrap(),
        None,
        None,
    )
    .unwrap();
    assert!(serde_json::to_vec(&payload).unwrap().len() as u64 <= budget);

    let wire = serde_json::to_vec(&json!({"id": 1})).unwrap();
    assert!(contract::validate_final_bytes(&wire, 0).is_err());
}

#[test]
fn compact_error_stays_within_its_own_budget_with_long_correlation_id() {
    let budget = vectors()["error_budget_bytes"].as_u64().unwrap();
    let correlation_id = format!("req-关联-{}", "a".repeat(512));

    let rule = &vectors()["compact_error_rule"];
    let required = rule["fields"]["required"].as_array().unwrap();
    let optional = rule["fields"]["optional"].as_array().unwrap();
    assert_eq!(optional.len(), 1);
    assert_eq!(optional[0], "actual_bytes");
    assert_eq!(rule["correlation_id_match"], "decoded_string_values");

    for id in [
        "error_data_not_dispatched",
        "error_data_unknown_without_size",
    ] {
        let item = vector(id);
        let payload = contract::make_compact_capacity_error(
            item["direction"].as_str().unwrap(),
            item["max_bytes"].as_u64().unwrap(),
            budget,
            &correlation_id,
            item["outcome"].as_str().unwrap(),
            item["actual_bytes"].as_u64(),
            item["message"].as_str(),
        )
        .unwrap();
        let wire = serde_json::to_vec(&payload).unwrap();
        let object = payload.as_object().unwrap();

        for field in required {
            assert!(object.contains_key(field.as_str().unwrap()));
        }
        assert_eq!(
            object.contains_key("actual_bytes"),
            item.get("actual_bytes").is_some()
        );
        assert!(contract::validate_final_bytes(&wire, budget).unwrap().fits);
        assert!(!String::from_utf8(wire).unwrap().contains(&correlation_id));
        assert_eq!(
            contract::parse_capacity_error_data(&object["data"]).unwrap(),
            object["data"]
        );
    }
}

#[test]
fn compact_error_rejects_escaped_correlation_id_echo() {
    let rule = &vectors()["compact_error_rule"];
    assert_eq!(rule["correlation_id_match"], "decoded_string_values");

    for item in vectors()["correlation_id_echo_vectors"].as_array().unwrap() {
        let correlation_id = item["correlation_id"].as_str().unwrap();
        let error = contract::make_compact_capacity_error(
            contract::DIRECTION_RESPONSE,
            65535,
            vectors()["error_budget_bytes"].as_u64().unwrap(),
            correlation_id,
            contract::OUTCOME_UNKNOWN,
            Some(70000),
            Some(correlation_id),
        )
        .expect_err("correlation IDs must not be echoed");
        assert_eq!(error.code, contract::ERROR_INVALID_CAPACITY_VALUE);
    }
}

#[test]
fn compact_error_rejects_oversized_serialization_without_recursion() {
    let item = vector("error_data_unknown_without_size");
    let error = contract::make_compact_capacity_error(
        item["direction"].as_str().unwrap(),
        item["max_bytes"].as_u64().unwrap(),
        1,
        "correlation-id",
        item["outcome"].as_str().unwrap(),
        None,
        None,
    )
    .expect_err("one byte cannot hold a structured error");
    assert_eq!(error.code, contract::ERROR_INVALID_CAPACITY_VALUE);
}

#[test]
fn unknown_outcome_is_default_for_oversized_response() {
    let data = contract::capacity_error_data(
        contract::DIRECTION_RESPONSE,
        65535,
        contract::OUTCOME_UNKNOWN,
        Some(70000),
    )
    .unwrap();

    assert_eq!(data["outcome"], contract::OUTCOME_UNKNOWN);
    assert_eq!(data["actual_bytes"], 70000);
}
