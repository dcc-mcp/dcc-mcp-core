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
