"""Tests for pure-Python ToolResult factory helpers."""

from __future__ import annotations

import json

from dcc_mcp_core.result_envelope import ToolResult


def test_tool_result_ok_puts_kwargs_in_context() -> None:
    result = ToolResult.ok("Loaded skill", name="recipe.x").to_dict()

    assert result == {
        "success": True,
        "message": "Loaded skill",
        "context": {"name": "recipe.x"},
    }


def test_tool_result_fail_sets_error_prompt_and_context() -> None:
    result = ToolResult.fail(
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
    assert ToolResult.not_found("Skill", "missing").to_dict() == {
        "success": False,
        "message": "Skill not found: missing",
        "error": "not_found",
    }
    assert ToolResult.invalid_input("Bad radius", radius=-1).to_dict() == {
        "success": False,
        "message": "Bad radius",
        "error": "invalid_input",
        "context": {"radius": -1},
    }


def test_tool_result_json_uses_pruned_wire_shape() -> None:
    payload = json.loads(ToolResult.ok("Done").to_json())

    assert payload == {"success": True, "message": "Done"}
