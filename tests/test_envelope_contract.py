"""Cross-producer contract tests for issue #2183."""

from __future__ import annotations

import json
from typing import Any

import pytest

from dcc_mcp_core import SerializeFormat
from dcc_mcp_core import from_exception
from dcc_mcp_core import serialize_result
from dcc_mcp_core import validate_action_result
from dcc_mcp_core._server.inprocess_executor import exception_to_error_envelope
from dcc_mcp_core._server.inprocess_executor import sandbox_denied_envelope
from dcc_mcp_core.result_envelope import ToolResultEnvelope
from dcc_mcp_core.script_execution import ScriptExecutionResult
from dcc_mcp_core.skill import skill_entry
from dcc_mcp_core.skill import skill_error
from dcc_mcp_core.skill import skill_error_with_trace
from dcc_mcp_core.skill import skill_exception
from dcc_mcp_core.skill import skill_success
from dcc_mcp_core.skill import skill_warning
from dcc_mcp_core.skills_helper import skill_error_from_exception

_CANONICAL_KEYS = {"success", "message", "error", "prompt", "context", "_meta"}


def _assert_canonical(payload: dict[str, Any]) -> None:
    assert set(payload) <= _CANONICAL_KEYS
    assert isinstance(payload["success"], bool)
    if "message" in payload:
        assert isinstance(payload["message"], str)
    if payload.get("error") is not None:
        assert isinstance(payload["error"], str)
    if payload.get("prompt") is not None:
        assert isinstance(payload["prompt"], str)
    if "context" in payload:
        assert isinstance(payload["context"], dict)
    if "_meta" in payload:
        assert isinstance(payload["_meta"], dict)
    normalized = ToolResultEnvelope.from_dict(payload).to_dict(prune_empty=False)
    for key, value in payload.items():
        assert normalized[key] == value
    assert ToolResultEnvelope.from_dict(normalized).to_dict(prune_empty=False) == normalized


def _captured_exception() -> RuntimeError:
    try:
        raise RuntimeError("host stopped")
    except RuntimeError as exc:
        return exc


def test_all_python_result_producers_share_one_envelope_schema() -> None:
    exc = _captured_exception()

    @skill_entry
    def missing_host(**_: Any) -> dict[str, Any]:
        raise ImportError("No module named 'zbrush'", name="zbrush")

    @skill_entry
    def interrupted(**_: Any) -> dict[str, Any]:
        raise KeyboardInterrupt("artist cancelled")

    payloads = [
        ToolResultEnvelope.ok("done", object_name="cube").to_dict(),
        ToolResultEnvelope.fail("failed", error="invalid_input").to_dict(),
        skill_success("done", object_name="cube"),
        skill_error("failed", "invalid_input"),
        skill_error_with_trace("failed", "execution_error", tb="Traceback: host stopped"),
        skill_warning("done with warning", warning="slow"),
        skill_exception(exc),
        skill_error_from_exception(exc, operation="inspect_scene"),
        from_exception("RuntimeError: host stopped").to_dict(),
        ScriptExecutionResult.from_exception(exc),
        exception_to_error_envelope(exc),
        sandbox_denied_envelope(PermissionError("blocked"), action_name="execute_python"),
        missing_host(),
        interrupted(),
    ]

    for payload in payloads:
        _assert_canonical(payload)


def test_inprocess_exception_uses_string_code_and_structured_meta() -> None:
    payload = exception_to_error_envelope(_captured_exception())

    assert payload["error"] == "RuntimeError"
    assert payload["_meta"]["dcc.error"]["type"] == "RuntimeError"
    assert payload["_meta"]["dcc.error"]["message"] == "host stopped"
    assert "Traceback" in payload["_meta"]["dcc.error"]["traceback"]


def test_exception_producers_keep_diagnostics_in_namespaced_meta() -> None:
    exc = _captured_exception()
    payloads = [
        skill_exception(exc),
        skill_error_from_exception(exc),
        from_exception("RuntimeError: host stopped").to_dict(),
        ScriptExecutionResult.from_exception(exc),
    ]

    for payload in payloads:
        assert isinstance(payload["error"], str)
        assert payload["_meta"]["dcc.error"]["type"] == "RuntimeError"
        assert payload["_meta"]["dcc.error"]["message"] == "host stopped"
        assert payload["_meta"]["dcc.error"]["traceback"]


@pytest.mark.parametrize(
    ("unit", "count", "expected_prefix_count"),
    [("错", 342, 333), ("🙂", 260, 250)],
)
def test_runtime_from_exception_truncates_utf8_on_character_boundary(
    unit: str,
    count: int,
    expected_prefix_count: int,
) -> None:
    payload = from_exception(unit * count).to_dict()
    traceback = payload["_meta"]["dcc.error"]["traceback"]

    assert traceback.startswith((unit * expected_prefix_count) + "...")
    assert "truncated, see trace_id" in traceback


@pytest.mark.parametrize(
    "message",
    [
        r"C:\scene.ma: access denied",
        "C:scene.ma: access denied",
        "https://example.invalid/file: failed",
        "dcc-mcp://host/tool: failed",
        "Could not open: file",
    ],
)
def test_runtime_from_exception_rejects_unstable_error_code_prefixes(message: str) -> None:
    payload = from_exception(message).to_dict()

    assert payload["error"] == "Exception"
    assert payload["_meta"]["dcc.error"]["message"] == message


def test_trace_helpers_merge_caller_meta_without_allowing_contract_override() -> None:
    caller_meta = {
        "vendor.trace": {"id": "trace-42"},
        "dcc.error": {"type": "Spoofed"},
        "dcc.raw_trace": {"underlying_call": "spoofed()"},
    }
    traced = skill_error_with_trace(
        "failed",
        "execution_error",
        underlying_call="maya.cmds.polyCube()",
        _meta=caller_meta,
    )
    caught = skill_exception(_captured_exception(), _meta=caller_meta)

    assert traced["_meta"]["vendor.trace"] == {"id": "trace-42"}
    assert traced["_meta"]["dcc.raw_trace"]["underlying_call"] == "maya.cmds.polyCube()"
    assert caught["_meta"]["vendor.trace"] == {"id": "trace-42"}
    assert caught["_meta"]["dcc.error"]["type"] == "RuntimeError"


def test_runtime_tool_result_preserves_meta_during_json_round_trip() -> None:
    payload = exception_to_error_envelope(_captured_exception())

    runtime_result = validate_action_result(payload)
    runtime_payload = runtime_result.to_dict()
    serialized_payload = json.loads(serialize_result(runtime_result, SerializeFormat.Json))

    assert runtime_payload["_meta"] == payload["_meta"]
    assert serialized_payload["_meta"] == payload["_meta"]
    assert "_meta" not in runtime_payload["context"]


@pytest.mark.parametrize(
    ("dcc_type", "asset_name"),
    [("maya", "hero_mesh"), ("photoshop", "beauty_comp")],
)
def test_envelope_context_is_host_agnostic(dcc_type: str, asset_name: str) -> None:
    payload = ToolResultEnvelope.ok(
        "Asset inspected",
        dcc_type=dcc_type,
        asset_name=asset_name,
    ).to_dict()

    _assert_canonical(payload)
    assert payload["context"] == {"dcc_type": dcc_type, "asset_name": asset_name}
