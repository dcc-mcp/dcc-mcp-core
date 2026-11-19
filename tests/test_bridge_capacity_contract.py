"""Tests for the shared bridge capacity contract (issue #2821)."""

from __future__ import annotations

import json
from pathlib import Path
import sys

import pytest

from dcc_mcp_core.wire import capacity as contract

VECTORS_PATH = Path(__file__).resolve().parents[1] / "contracts" / "bridge-capacity-v1.conformance.json"


def load_vectors():
    with VECTORS_PATH.open(encoding="utf-8") as stream:
        return json.load(stream)


VECTORS = load_vectors()


def find_vector(vector_id):
    return next(item for item in VECTORS["vectors"] if item["id"] == vector_id)


def test_exact_limit_and_limit_plus_one_use_final_utf8_bytes():
    exact = find_vector("exact_limit_ascii")
    over = find_vector("limit_plus_one_ascii")

    exact_result = contract.validate_final_bytes(exact["payload"], exact["limit_bytes"])
    over_result = contract.validate_final_bytes(over["payload"], over["limit_bytes"])

    assert exact_result.actual_bytes == exact["expected_bytes"]
    assert exact_result.fits is exact["fits"]
    assert over_result.actual_bytes == over["expected_bytes"]
    assert over_result.fits is over["fits"]


def test_cjk_emoji_and_json_escapes_count_serialized_bytes():
    cjk = find_vector("cjk_and_emoji")
    escapes = find_vector("json_escapes")

    assert contract.validate_final_bytes(cjk["payload"], cjk["limit_bytes"]).actual_bytes == cjk["expected_bytes"]
    assert (
        contract.validate_final_bytes(escapes["payload"], escapes["limit_bytes"]).actual_bytes
        == escapes["expected_bytes"]
    )


def test_long_id_and_envelope_overhead_use_final_serialized_bytes():
    vector = find_vector("long_request_id_envelope_overhead")
    message = vector["message"]
    raw = contract.serialized_json_bytes(message)

    assert len(raw) >= vector["min_bytes"]
    assert vector["message"]["id"] in raw.decode("utf-8")
    assert contract.validate_final_bytes(raw, vector["limit_bytes"]).fits is vector["fits"]

    smaller_limit = contract.validate_final_bytes(raw, 64)
    assert smaller_limit.fits is False
    assert smaller_limit.actual_bytes == len(raw)


def test_already_serialized_bytes_are_not_re_encoded():
    raw = json.dumps({"id": "模型🙂", "quote": 'a"b'}, ensure_ascii=False).encode("utf-8")
    budget = contract.validate_final_bytes(raw, len(raw))

    assert budget.actual_bytes == len(raw)
    assert budget.fits is True


def test_capacity_policy_rejects_booleans_and_invalid_bounds():
    for bad in (True, False):
        with pytest.raises(contract.CapacityContractError) as raised:
            contract.CapacityPolicy(bad, VECTORS["min_receive_bytes"])
        assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE

        with pytest.raises(contract.CapacityContractError) as raised:
            contract.CapacityPolicy(VECTORS["min_receive_bytes"], bad)
        assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE

    with pytest.raises(contract.CapacityContractError) as raised:
        contract.CapacityPolicy(0, VECTORS["min_receive_bytes"])
    assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE

    with pytest.raises(contract.CapacityContractError) as raised:
        contract.CapacityPolicy(VECTORS["min_receive_bytes"] + 1, VECTORS["min_receive_bytes"])
    assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE


def test_directional_declaration_legacy_and_policy_clamping():
    by_id = {item["id"]: item for item in VECTORS["vectors"]}

    for vector_id in ("legacy_flat_declaration", "asymmetric_directional_declaration", "only_one_direction_declared"):
        vector = by_id[vector_id]
        state = contract.parse_capacity_declaration(vector["capabilities"])
        assert state.negotiation == vector["expected_negotiation"]
        assert state.receive.to_dict() == vector["expected_receive"]

    for vector_id in ("missing_declaration", "legacy_alternate_key"):
        vector = by_id[vector_id]
        state = contract.parse_capacity_declaration(vector["capabilities"])
        assert state.negotiation == contract.LEGACY_ASSUMED
        assert state.receive.to_dict() == vector["expected_receive"]

    huge = by_id["huge_declaration_is_clamped"]
    policy = contract.CapacityPolicy(huge["policy"]["min_bytes"], huge["policy"]["max_bytes"])
    huge_state = contract.parse_capacity_declaration(huge["capabilities"], policy)
    assert huge_state.receive.to_dict() == huge["expected_receive"]

    small = by_id["small_declaration_is_clamped_up"]
    small_state = contract.parse_capacity_declaration(small["capabilities"])
    assert small_state.receive.to_dict() == small["expected_receive"]


def test_invalid_declarations_are_rejected_deterministically():
    vectors = [item for item in VECTORS["vectors"] if item["kind"] == "invalid_capability"]
    assert len(vectors) >= 6

    for vector in vectors:
        with pytest.raises(contract.CapacityContractError) as raised:
            contract.parse_capacity_declaration(vector["capabilities"])
        assert raised.value.code == vector["expected_error"]


def test_structured_error_data_round_trips_without_payload_echo():
    for vector in [item for item in VECTORS["vectors"] if item["kind"] == "error_data"]:
        data = contract.make_capacity_error_data(
            vector["direction"],
            vector["max_bytes"],
            vector["outcome"],
            vector.get("actual_bytes"),
        )
        wire = json.dumps(data)
        decoded = contract.parse_capacity_error_data(json.loads(wire))

        assert decoded == data
        assert "content" not in decoded
        assert decoded["outcome"] == vector["outcome"]
        assert decoded.get("actual_bytes") == vector.get("actual_bytes")


def test_unknown_outcome_is_default_for_oversized_response():
    data = contract.make_capacity_error_data(contract.DIRECTION_RESPONSE, 65535, actual_bytes=70000)

    assert data["outcome"] == contract.OUTCOME_UNKNOWN
    assert data["actual_bytes"] == 70000


def test_structured_error_data_rejects_invalid_fields_consistently():
    valid = {"direction": "request", "max_bytes": 100, "outcome": "unknown"}

    invalid = [
        dict(valid, direction="sideways"),
        dict(valid, outcome="indeterminate"),
        {"direction": "request", "outcome": "unknown"},
        dict(valid, max_bytes="100"),
        dict(valid, max_bytes=0),
    ]

    for data in invalid:
        with pytest.raises(contract.CapacityContractError) as raised:
            contract.parse_capacity_error_data(data)
        assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE


def test_invalid_error_data_vectors_are_rejected():
    vectors = [item for item in VECTORS["vectors"] if item["kind"] == "invalid_error_data"]
    assert len(vectors) >= 2

    for vector in vectors:
        data = {
            "direction": vector["direction"],
            "max_bytes": vector["max_bytes"],
            "outcome": vector["outcome"],
        }
        if "actual_bytes" in vector:
            data["actual_bytes"] = vector["actual_bytes"]
        with pytest.raises(contract.CapacityContractError) as raised:
            contract.parse_capacity_error_data(data)
        assert raised.value.code == vector["expected_error"]


def test_compact_error_stays_within_its_own_budget_with_long_correlation_id():
    vector = find_vector("error_data_not_dispatched")
    correlation_id = "req-" + "关联-" + "a" * 512
    payload = contract.make_compact_capacity_error(
        vector["direction"],
        vector["max_bytes"],
        VECTORS["error_budget_bytes"],
        correlation_id,
        vector["outcome"],
        vector["actual_bytes"],
        vector["message"],
    )
    wire = contract.serialized_json_bytes(payload)

    assert tuple(payload) == tuple(VECTORS["compact_error_rule"]["fields"])
    assert contract.validate_final_bytes(wire, VECTORS["error_budget_bytes"]).fits is True
    assert correlation_id not in wire.decode("utf-8")
    assert contract.parse_capacity_error_data(payload["data"]) == payload["data"]


def test_compact_error_rejects_oversized_serialization_without_recursion():
    vector = find_vector("error_data_unknown_without_size")
    with pytest.raises(contract.CapacityContractError) as raised:
        contract.make_compact_capacity_error(
            vector["direction"],
            vector["max_bytes"],
            1,
            "correlation-id",
            vector["outcome"],
        )
    assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE


def test_compact_error_rejects_echoed_correlation_id():
    with pytest.raises(contract.CapacityContractError) as raised:
        contract.make_compact_capacity_error(
            contract.DIRECTION_RESPONSE,
            65535,
            VECTORS["error_budget_bytes"],
            "leak-me",
            contract.OUTCOME_UNKNOWN,
            70000,
            "leak-me",
        )
    assert raised.value.code == contract.ERROR_INVALID_CAPACITY_VALUE
