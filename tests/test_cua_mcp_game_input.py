"""B3 conformance and actual Python bridge tests; no physical input or Host."""

from __future__ import annotations

from copy import deepcopy
from dataclasses import replace
import hashlib
import json
from pathlib import Path

import pytest

from dcc_mcp_core.adapter_contracts import UiActionRequest
from dcc_mcp_core.adapter_contracts import UiControlPolicy
from dcc_mcp_core.cua_cli import CuaCliError
from dcc_mcp_core.host.cua_mcp_game_capability import GAME_CAPABILITY_KEY
from dcc_mcp_core.host.cua_mcp_game_capability import GAME_DESCRIPTOR
from dcc_mcp_core.host.cua_mcp_game_capability import require_game_capability
from dcc_mcp_core.host.cua_mcp_game_input import game_input_from_params
from dcc_mcp_core.host.cua_mcp_game_input import game_input_payload
from dcc_mcp_core.host.cua_mcp_game_input import normalize_game_keys
from dcc_mcp_core.host.cua_mcp_game_schema import game_call_condition
from test_cua_mcp_pixels import SCRIPTS
from test_cua_mcp_pixels import TARGET
from test_cua_mcp_pixels import client
from test_cua_mcp_pixels import runtime

FIXTURES = Path(__file__).parent / "fixtures/cua_game_input"
CONTRACT = json.loads((FIXTURES / "game_b3.v1.json").read_text(encoding="utf-8"))
GRANTS = ("game_navigation", "relative_mouse")


def advertise(process):
    process.mutate_initialize = lambda raw: raw["capabilities"].update(
        experimental={GAME_CAPABILITY_KEY: deepcopy(GAME_DESCRIPTOR)}
    )

    def catalog(raw):
        raw["tools"][1]["inputSchema"]["allOf"] = [game_call_condition()]
        schema = raw["tools"][0]["inputSchema"]
        schema["allOf"] = [
            {
                "if": {"properties": {"observation_mode": {"const": "pixels_only"}}, "required": ["observation_mode"]},
                "then": {
                    "properties": {
                        "allowed_actions": {
                            "items": {
                                "oneOf": [
                                    {
                                        "type": "object",
                                        "additionalProperties": False,
                                        "required": ["action", "input_kind", "authorization_category", "secret_input"],
                                        "properties": {
                                            "action": {"type": "string", "enum": list(GRANTS)},
                                            "input_kind": {"const": "raw_input"},
                                            "authorization_category": {"const": "raw_input"},
                                            "secret_input": {"const": False},
                                        },
                                    }
                                ]
                            }
                        }
                    }
                },
            }
        ]

    process.mutate_catalog = catalog


def test_exact_shared_fixture_and_descriptor_pins():
    assert hashlib.sha256((FIXTURES / "game_b3.v1.json").read_bytes()).hexdigest() == (
        "991353176803e194a9fa47a769736fc8b77fa6cf1a992a4463add17c06d9bc31"
    )
    assert hashlib.sha256((FIXTURES / "capability.json").read_bytes()).hexdigest() == (
        "ce3ce401af78655473c04fa437060165562594f5ef10a556d09bcaa7b402d427"
    )
    require_game_capability(
        {"experimental": {GAME_CAPABILITY_KEY: json.loads((FIXTURES / "capability.json").read_text(encoding="utf-8"))}},
        GRANTS,
    )


@pytest.mark.parametrize("case", CONTRACT["cases"], ids=lambda c: c["name"])
def test_shared_canonical_wire_conformance(case):
    original = deepcopy(case["payload"])
    if case["accept"]:
        result = game_input_payload(original, tuple(case["grants"]))
        expected = dict(original)
        if original["action"] == "game_navigation":
            expected.setdefault("duration_ms", 0)
        assert result == expected
        assert original == case["payload"]
    else:
        with pytest.raises(CuaCliError):
            game_input_payload(original, tuple(case["grants"]))


@pytest.mark.parametrize("case", [c for c in CONTRACT["cases"] if "core_frontend_keys" in c], ids=lambda c: c["name"])
def test_shared_frontend_alias_cases(case):
    if case["accept"]:
        assert normalize_game_keys(case["core_frontend_keys"]) == case["expected_canonical_keys"]
    else:
        with pytest.raises(CuaCliError):
            normalize_game_keys(case["core_frontend_keys"])


@pytest.mark.parametrize("keys", [[" W + leftShift "], ["leftcontrol+e"], ["space"]])
def test_frontend_normalizes_only_documented_ascii_keys(keys):
    assert all(key in CONTRACT["canonical_keys"] for key in normalize_game_keys(keys))


@pytest.mark.parametrize(
    "keys", [["W+"], ["+W"], ["W++F"], ["\u017f"], ["\uff37"], ["W\u00a0"], ["SPACEBAR"], ["LEFT_CTRL"], ["W+w"]]
)
def test_frontend_rejects_empty_non_ascii_and_alias_duplicates(keys):
    with pytest.raises(CuaCliError):
        normalize_game_keys(keys)


@pytest.mark.parametrize(
    "extra", [{"unknown": None}, {"profile": "game_b3.v1"}, {"x": None}, {"path": []}, {"button": None}]
)
def test_frontend_never_discards_unknown_or_inapplicable_fields(extra):
    with pytest.raises(CuaCliError):
        game_input_from_params(
            {"action": "game_navigation", "intent": "game_navigation", "keys": ["W"], **extra}, GRANTS
        )


@pytest.mark.parametrize(
    "fault", ["missing", "profile", "platform", "extra", "bool_min", "float_max", "int_bool", "null"]
)
def test_exact_typed_descriptor_rejection_precedes_task(runtime, fault):
    config, process, _ = runtime
    advertise(process)
    descriptor = deepcopy(GAME_DESCRIPTOR)
    if fault == "missing":
        descriptor.pop("contract")
    elif fault in {"profile", "platform"}:
        descriptor[fault] = "wrong"
    elif fault == "extra":
        descriptor["new_field"] = True
    elif fault == "bool_min":
        descriptor["limits"]["min_keys"] = True
    elif fault == "float_max":
        descriptor["limits"]["max_keys"] = 4.0
    elif fault == "int_bool":
        descriptor["shape"]["relative_buttonless"] = 1
    else:
        descriptor = None
    process.mutate_initialize = lambda raw: raw["capabilities"].update(experimental={GAME_CAPABILITY_KEY: descriptor})
    with pytest.raises(CuaCliError) as error:
        client(replace(config, allowed_actions=GRANTS))
    assert error.value.code == "unsupported"
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)
    assert process.returncode == 0


def test_legacy_catalog_cannot_authorize_new_scopes(runtime):
    config, process, _ = runtime
    advertise(process)
    process.mutate_catalog = lambda raw: None
    with pytest.raises(CuaCliError) as error:
        client(replace(config, allowed_actions=GRANTS))
    assert error.value.code == "unsupported"
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


def test_inner_host_typed_unsupported_survives_startup(runtime):
    config, process, _ = runtime
    advertise(process)
    process.mutate_open = lambda raw: raw.update(ok=False, code="unsupported", message="Missing inner capability")
    with pytest.raises(CuaCliError) as error:
        client(replace(config, allowed_actions=GRANTS))
    assert error.value.code == "unsupported"
    assert process.returncode == 0


@pytest.mark.parametrize("keyboard,relative,expected", [(True, True, True), (True, False, False), (False, True, False)])
def test_combined_request_requires_both_policy_permissions(keyboard, relative, expected):
    policy = UiControlPolicy(allow_keyboard_shortcuts=keyboard, allow_raw_coordinates=relative)
    assert policy.allows_request(UiActionRequest(None, "game_navigation", dx=1, dy=0)) is expected


@pytest.mark.parametrize(
    "action",
    [
        {"action": "game_navigation", "keys": ["w+LeftShift"], "duration_ms": 250, "dx": 32, "dy": -8},
        {"action": "game_navigation", "keys": ["W+SHIFT+F"], "duration_ms": 0, "dx": 1, "dy": 0},
        {"action": "game_navigation", "keys": ["Z"]},
        {"action": "relative_mouse", "dx": -256, "dy": 256},
    ],
)
def test_formal_skill_bridge_preserves_one_combined_native_transaction(runtime, monkeypatch, action):
    from dcc_mcp_core._server.inprocess_executor import HostExecutionBridge

    config, process, _ = runtime
    advertise(process)
    config = replace(config, allowed_actions=GRANTS)
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_PROCESS_ID", str(TARGET["process_id"]))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_WINDOW_HANDLE", str(TARGET["window_handle"]))
    monkeypatch.setenv("DCC_MCP_UI_CONTROL_WINDOW_TITLE", TARGET["window_title"])
    monkeypatch.setenv("DCC_MCP_CUA_ALLOW_RAW_INPUT", "true")
    monkeypatch.delenv("DCC_MCP_UI_CONTROL_PROCESS_NAME", raising=False)
    bridge = HostExecutionBridge()

    def call(name, arguments):
        return bridge.execute_script(
            str(SCRIPTS / (name + ".py")),
            {"session_id": "b3-contract", **arguments},
            skill_name="ui-control",
            trusted_adapter_scope={"dcc_type": "unreal", **TARGET},
            trusted_ui_control_runtime=config,
        )

    try:
        snapshot = call("snapshot", {})
        assert snapshot["success"] is True
        request = {**action, "intent": "game_navigation", "snapshot_id": snapshot["context"]["snapshot_id"]}
        result = call("act", request)
        assert result["success"] is True, result
        attempts = [
            r for r in process.requests if r.get("params", {}).get("arguments", {}).get("method") == "execute_action"
        ]
        assert len(attempts) == 1
        wire = attempts[0]["params"]["arguments"]["params"]["action"]
        assert wire == game_input_from_params(request, GRANTS)
        assert result["context"]["result"]["metadata"]["effect"] == "unverifiable"
        assert call("act", request)["error"] == "stale_observation"
    finally:
        bridge.shutdown_script_execution()


def test_incomplete_game_delivery_blocks_next_mutation_without_release_claim(runtime):
    config, process, _ = runtime
    advertise(process)
    owned = client(replace(config, allowed_actions=GRANTS))
    owned.snapshot(max_depth=1, max_nodes=1)

    def incomplete(raw):
        raw["success"] = False
        raw["result"]["success"] = False
        raw["result"]["delivery"]["delivery_completed"] = False
        raw["result"]["delivery"]["cleanup_inserted_events"] = 0

    process.mutate_action = incomplete
    result = owned.execute(
        game_input_from_params({"action": "game_navigation", "intent": "game_navigation", "keys": ["W"]}, GRANTS)
    )
    assert result["success"] is False
    assert result["result"]["delivery"]["cleanup_inserted_events"] == 0
    assert "released" not in result
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=1, max_nodes=1)
    assert process.returncode == 0


@pytest.mark.parametrize(
    "change",
    [{"duration_ms": None}, {"keys": ["CTRL+LCTRL"]}, {"dx": 1}, {"profile": "game_b3.v1"}, {"intent": "navigate"}],
)
def test_invalid_frontend_does_not_start_runtime(runtime, change):
    import importlib.util

    config, process, launches = runtime
    path = SCRIPTS / "_cua_backend.py"
    spec = importlib.util.spec_from_file_location("b3_frontend_rejection", path)
    backend = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(backend)
    result = backend.act_tool(
        {
            "action": "game_navigation",
            "intent": "game_navigation",
            "keys": ["W"],
            "trusted_ui_control_runtime": replace(config, allowed_actions=GRANTS),
            **change,
        }
    )
    assert result["success"] is False
    assert result["error"] == "invalid_action"
    assert launches == [] and process.requests == []


@pytest.mark.parametrize("extra", [{"dx": 1, "dy": 0}, {"intent": "game_navigation"}])
def test_legacy_route_cannot_silently_drop_new_game_semantics(runtime, extra):
    import importlib.util

    _, process, launches = runtime
    spec = importlib.util.spec_from_file_location("b3_legacy_rejection", SCRIPTS / "_cua_game_input.py")
    module = importlib.util.module_from_spec(spec)
    spec.loader.exec_module(module)
    with pytest.raises(CuaCliError, match="owner-selected"):
        module.prepare({"action": "game_navigation", "keys": ["W"], **extra})
    assert launches == [] and process.requests == []


def test_combined_grant_failure_occurs_before_dispatch(runtime):
    config, process, _ = runtime
    advertise(process)
    owned = client(replace(config, allowed_actions=("game_navigation",)))
    owned.snapshot(max_depth=1, max_nodes=1)
    action = game_input_from_params(
        {"action": "game_navigation", "intent": "game_navigation", "keys": ["W"], "dx": 1, "dy": 0}, GRANTS
    )
    with pytest.raises(CuaCliError) as error:
        owned.execute(action)
    assert error.value.code == "unsupported_action"
    assert not any(r.get("params", {}).get("arguments", {}).get("method") == "execute_action" for r in process.requests)
    owned.stop()


def test_game_timeout_does_not_replay_or_authorize_another_transaction(runtime):
    config, process, _ = runtime
    advertise(process)
    config = replace(config, allowed_actions=GRANTS)
    owned = client(config)
    owned.snapshot(max_depth=1, max_nodes=1)
    object.__setattr__(config, "timeout_seconds", 0.01)
    process.drop_response = True
    action = game_input_from_params(
        {
            "action": "game_navigation",
            "intent": "game_navigation",
            "keys": ["W+SHIFT"],
            "duration_ms": 500,
            "dx": 256,
            "dy": 0,
        },
        GRANTS,
    )
    with pytest.raises(CuaCliError, match="not retried"):
        owned.execute(action)
    with pytest.raises(CuaCliError):
        owned.snapshot(max_depth=1, max_nodes=1)
    attempts = [
        r for r in process.requests if r.get("params", {}).get("arguments", {}).get("method") == "execute_action"
    ]
    assert len(attempts) == 1


@pytest.mark.parametrize(
    "change",
    [
        {"button": None},
        {"x": 1},
        {"duration_ms": None},
        {"duration_ms": 501},
        {"dx": 1},
        {"dx": 0, "dy": 0},
        {"profile": "game_b3.v1"},
    ],
)
def test_public_game_schema_rejects_malformed_request(change):
    import jsonschema

    from dcc_mcp_core import parse_skill_md

    meta = parse_skill_md(str(SCRIPTS.parent))
    schema = json.loads(next(t for t in meta.tools if t.name == "act").input_schema)
    with pytest.raises(jsonschema.ValidationError):
        jsonschema.validate({"action": "game_navigation", "intent": "game_navigation", "keys": ["W"], **change}, schema)


@pytest.mark.parametrize(
    "malformed",
    [
        None,
        {},
        [None],
        [
            {
                "if": {"properties": {"observation_mode": {"const": "pixels_only"}}, "required": ["observation_mode"]},
                "then": None,
            }
        ],
    ],
)
def test_malformed_action_schema_is_typed_unsupported(runtime, malformed):
    config, process, _ = runtime
    advertise(process)
    process.mutate_catalog = lambda raw: raw["tools"][0]["inputSchema"].update(allOf=malformed)
    with pytest.raises(CuaCliError) as error:
        client(replace(config, allowed_actions=GRANTS))
    assert error.value.code == "unsupported"
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


@pytest.mark.parametrize(
    "fault", ["absent", "open", "wrong_intent", "wrong_limit", "bool_limit", "no_pairs", "extra_key", "no_observation"]
)
def test_closed_action_branch_required_before_start(runtime, fault):
    config, process, _ = runtime
    advertise(process)
    original = process.mutate_catalog

    def corrupt(raw):
        original(raw)
        schema = raw["tools"][1]["inputSchema"]
        if fault == "absent":
            schema.pop("allOf")
            return
        params = schema["allOf"][0]["then"]["properties"]["params"]
        action = params["properties"]["action"]["oneOf"][0]
        if fault == "open":
            action["additionalProperties"] = True
        elif fault == "wrong_intent":
            action["properties"]["intent"]["const"] = "navigate"
        elif fault == "wrong_limit":
            action["properties"]["duration_ms"]["maximum"] = 501
        elif fault == "bool_limit":
            action["properties"]["keys"]["minItems"] = True
        elif fault == "no_pairs":
            action.pop("dependentRequired")
        elif fault == "extra_key":
            action["properties"]["keys"]["items"]["enum"].append("UP")
        else:
            params["required"].remove("observation_id")

    process.mutate_catalog = corrupt
    with pytest.raises(CuaCliError) as error:
        client(replace(config, allowed_actions=GRANTS))
    assert error.value.code == "unsupported"
    assert not any(r.get("params", {}).get("name") == "start_task" for r in process.requests)


@pytest.mark.parametrize(
    "case",
    [c for c in CONTRACT["cases"] if c.get("reason") != "missing_grant" and c["name"] != "duration_float"],
    ids=lambda c: c["name"],
)
def test_negotiated_action_schema_conforms_to_shared_wire_shapes(case):
    import jsonschema

    # JSON Schema regards 1.0 as an integer; runtime DTO validation additionally
    # rejects floats. Owner grant validation is separate from payload shape.
    schema = game_call_condition()["then"]["properties"]["params"]["properties"]["action"]
    errors = list(jsonschema.Draft202012Validator(schema).iter_errors(case["payload"]))
    assert (not errors) is case["accept"], errors
