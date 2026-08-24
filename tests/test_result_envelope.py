"""Tests for pure-Python ToolResult factory helpers."""

from __future__ import annotations

import json

import pytest

import dcc_mcp_core
from dcc_mcp_core.result_envelope import ToolResultEnvelope


def test_tool_result_ok_puts_kwargs_in_context() -> None:
    result = ToolResultEnvelope.ok("Loaded skill", name="recipe.x").to_dict()

    assert result == {
        "success": True,
        "message": "Loaded skill",
        "context": {"name": "recipe.x"},
    }


def test_tool_result_fail_sets_error_prompt_and_context() -> None:
    result = ToolResultEnvelope.fail(
        "Unknown tool",
        error="not_found",
        prompt="Call search_tools first.",
        tool_slug="maya.abc.missing",
    ).to_dict()

    assert result["success"] is False
    assert result["message"] == "Unknown tool"
    assert result["error"] == "not_found"
    assert result["prompt"] == "Call search_tools first."
    assert result["context"] == {"tool_slug": "maya.abc.missing"}


def test_tool_result_shortcut_factories() -> None:
    assert ToolResultEnvelope.not_found("Skill", "missing").to_dict() == {
        "success": False,
        "message": "Skill not found: missing",
        "error": "not_found",
    }
    assert ToolResultEnvelope.invalid_input("Bad radius", radius=-1).to_dict() == {
        "success": False,
        "message": "Bad radius",
        "error": "invalid_input",
        "context": {"radius": -1},
    }


def test_tool_result_json_uses_pruned_wire_shape() -> None:
    payload = json.loads(ToolResultEnvelope.ok("Done").to_json())

    assert payload == {"success": True, "message": "Done"}


def test_tool_result_envelope_preserves_namespaced_meta() -> None:
    payload = ToolResultEnvelope.fail(
        "Execution failed",
        error="execution_error",
        _meta={"dcc.error": {"type": "RuntimeError", "message": "boom"}},
    ).to_dict()

    assert payload["error"] == "execution_error"
    assert payload["_meta"]["dcc.error"]["type"] == "RuntimeError"


def test_tool_result_envelope_round_trips_canonical_shape() -> None:
    payload = {
        "success": False,
        "message": "Execution failed",
        "error": "execution_error",
        "prompt": "Retry after inspecting the error details.",
        "context": {"action_name": "create_sphere"},
        "_meta": {"dcc.error": {"type": "RuntimeError", "message": "boom"}},
    }

    assert ToolResultEnvelope.from_dict(payload).to_dict() == payload


def test_legacy_module_tool_result_alias_is_deprecated() -> None:
    from dcc_mcp_core import result_envelope

    with pytest.warns(DeprecationWarning, match="ToolResultEnvelope"):
        legacy = result_envelope.ToolResult

    assert legacy is not ToolResultEnvelope
    assert issubclass(legacy, ToolResultEnvelope)
    assert legacy(True).prompt == ""
    assert legacy.ok("Done", prompt="Inspect").to_dict() == {
        "success": True,
        "message": "Done",
        "context": {"prompt": "Inspect"},
    }


def test_runtime_model_and_wire_envelope_have_distinct_public_names() -> None:
    assert dcc_mcp_core.ToolResultEnvelope is ToolResultEnvelope
    assert dcc_mcp_core.ToolResult is not ToolResultEnvelope


def test_tool_result_envelope_rejects_object_error_on_direct_construction() -> None:
    with pytest.raises(TypeError, match=r"error.*string"):
        ToolResultEnvelope.fail("failed", error={"type": "RuntimeError"})  # type: ignore[arg-type]


@pytest.mark.parametrize("verified", [1, "yes"])
def test_tool_result_envelope_rejects_non_boolean_verification(verified: object) -> None:
    with pytest.raises(TypeError, match=r"verified.*bool"):
        ToolResultEnvelope.ok("done", verified=verified)  # type: ignore[arg-type]


def test_tool_result_envelope_rejects_conflicting_verification_evidence() -> None:
    with pytest.raises(ValueError, match="conflicts with postcondition"):
        ToolResultEnvelope.ok("done", postcondition={"verified": True}, verified=False)


def test_tool_result_envelope_does_not_mask_invalid_explicit_verification() -> None:
    with pytest.raises(TypeError, match=r"verified.*bool"):
        ToolResultEnvelope.ok("done", postcondition={"verified": 1}, verified=True)


def test_tool_result_envelope_rejects_explicit_null_verification() -> None:
    with pytest.raises(TypeError, match=r"verified.*bool"):
        ToolResultEnvelope.from_dict(
            {
                "success": True,
                "message": "done",
                "postcondition": {"verified": None},
            }
        )
