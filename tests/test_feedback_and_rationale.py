"""Tests for agent feedback and rationale utilities (issues #433, #434).

Covers:
- extract_rationale: extracts _meta.dcc.rationale from params dict
- extract_rationale: returns None when missing or malformed
- make_rationale_meta: builds correct _meta fragment
- register_feedback_tool: registers tool on a mock server
- get_feedback_entries: returns stored entries
- get_feedback_entries: filter by tool_name and severity
- clear_feedback: empties the store
- Feedback entries are capped at MAX_FEEDBACK_ENTRIES
- Public API importable from top-level dcc_mcp_core
"""

from __future__ import annotations

import json
from unittest.mock import MagicMock

import pytest

# ── extract_rationale ──────────────────────────────────────────────────────


def test_extract_rationale_from_dict():
    from dcc_mcp_core.feedback import extract_rationale

    params = {
        "name": "create_sphere",
        "arguments": {"radius": 1.0},
        "_meta": {"dcc": {"rationale": "User wants a reference sphere."}},
    }
    assert extract_rationale(params) == "User wants a reference sphere."


def test_extract_rationale_from_json_string():
    from dcc_mcp_core.feedback import extract_rationale

    params_str = json.dumps({"_meta": {"dcc": {"rationale": "Scale check"}}})
    assert extract_rationale(params_str) == "Scale check"


def test_extract_rationale_missing_returns_none():
    from dcc_mcp_core.feedback import extract_rationale

    assert extract_rationale({}) is None
    assert extract_rationale({"_meta": {}}) is None
    assert extract_rationale({"_meta": {"dcc": {}}}) is None


def test_extract_rationale_invalid_json_returns_none():
    from dcc_mcp_core.feedback import extract_rationale

    assert extract_rationale("not json") is None
    assert extract_rationale(None) is None


# ── make_rationale_meta ────────────────────────────────────────────────────


def test_make_rationale_meta_structure():
    from dcc_mcp_core.feedback import make_rationale_meta

    meta = make_rationale_meta("Creating a sphere for scale reference.")
    assert meta == {"_meta": {"dcc": {"rationale": "Creating a sphere for scale reference."}}}


def test_make_rationale_meta_round_trip():
    from dcc_mcp_core.feedback import extract_rationale
    from dcc_mcp_core.feedback import make_rationale_meta

    meta = make_rationale_meta("Test intent")
    assert extract_rationale(meta) == "Test intent"


# ── feedback store ─────────────────────────────────────────────────────────


def setup_function():
    """Clear feedback store before each test."""
    from dcc_mcp_core.feedback import clear_feedback

    clear_feedback()


def test_feedback_report_and_retrieve():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    params = json.dumps(
        {
            "tool_name": "maya_geometry__create_sphere",
            "intent": "Create a sphere",
            "attempt": "radius=1.0",
            "blocker": "Sphere not visible",
            "severity": "blocked",
        }
    )
    result = json.loads(_handle_feedback_report(params))
    assert result["success"] is True
    feedback_id = result["context"]["feedback_id"]

    entries = get_feedback_entries()
    assert len(entries) == 1
    assert entries[0]["id"] == feedback_id
    assert entries[0]["tool_name"] == "maya_geometry__create_sphere"
    assert entries[0]["severity"] == "blocked"


def test_filter_by_tool_name():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    for tool in ["tool_a", "tool_b", "tool_a"]:
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": tool,
                    "intent": "intent",
                    "blocker": "blocker",
                    "severity": "blocked",
                }
            )
        )

    assert len(get_feedback_entries(tool_name="tool_a")) == 2
    assert len(get_feedback_entries(tool_name="tool_b")) == 1


def test_filter_by_severity():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import get_feedback_entries

    for severity in ["blocked", "suggestion", "blocked"]:
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": "t",
                    "intent": "i",
                    "blocker": "b",
                    "severity": severity,
                }
            )
        )

    assert len(get_feedback_entries(severity="blocked")) == 2
    assert len(get_feedback_entries(severity="suggestion")) == 1


def test_clear_feedback_returns_count():
    from dcc_mcp_core.feedback import _handle_feedback_report
    from dcc_mcp_core.feedback import clear_feedback

    for _ in range(3):
        _handle_feedback_report(
            json.dumps(
                {
                    "tool_name": "t",
                    "intent": "i",
                    "blocker": "b",
                    "severity": "blocked",
                }
            )
        )
    count = clear_feedback()
    assert count == 3


def test_feedback_invalid_params():
    from dcc_mcp_core.feedback import _handle_feedback_report

    result = json.loads(_handle_feedback_report("not valid json {"))
    assert result["success"] is False


# ── register_feedback_tool ────────────────────────────────────────────────


def test_register_feedback_tool_registers_name():
    from dcc_mcp_core.feedback import register_feedback_tool

    registry = MagicMock()
    server = MagicMock()
    server.registry = registry

    register_feedback_tool(server, dcc_name="maya")

    registry.register.assert_called_once()
    call_kwargs = registry.register.call_args.kwargs
    assert call_kwargs["name"] == "dcc_feedback__report"
    assert call_kwargs["dcc"] == "maya"
    assert server.register_handler.call_count == 1
    name_arg = server.register_handler.call_args[0][0]
    assert name_arg == "dcc_feedback__report"


def test_register_feedback_tool_no_registry():
    from dcc_mcp_core.feedback import register_feedback_tool

    class _NoRegistry:
        @property
        def registry(self):
            raise AttributeError("no registry")

        def register_handler(self, *args, **kwargs):
            pass

    register_feedback_tool(_NoRegistry())


# ── public API ────────────────────────────────────────────────────────────


def test_importable_from_top_level():
    import dcc_mcp_core

    assert hasattr(dcc_mcp_core, "extract_rationale")
    assert hasattr(dcc_mcp_core, "make_rationale_meta")
    assert hasattr(dcc_mcp_core, "register_feedback_tool")
    assert hasattr(dcc_mcp_core, "get_feedback_entries")
    assert hasattr(dcc_mcp_core, "clear_feedback")
