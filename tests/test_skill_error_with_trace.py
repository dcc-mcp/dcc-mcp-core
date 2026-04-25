"""Tests for skill_error_with_trace helper (issue #427).

Covers:
- Basic contract: returns dict with success=False
- _meta.dcc.raw_trace block is present when trace fields are given
- _meta block is absent when no trace fields are given
- underlying_call is truncated to 500 chars
- All individual trace fields are optional
- skill_error_with_trace is importable from top-level dcc_mcp_core
- context kwargs are preserved alongside _meta
"""

from __future__ import annotations

import pytest


def test_basic_contract():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace("Something failed", "RuntimeError: oops")
    assert result["success"] is False
    assert result["message"] == "Something failed"
    assert result["error"] == "RuntimeError: oops"


def test_raw_trace_block_present():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace(
        "Failed to create sphere",
        "RuntimeError: radius must be > 0",
        underlying_call="maya.cmds.polySphere(name='x', radius=-1.0)",
        recipe_hint="references/RECIPES.md#create_sphere",
        introspect_hint="dcc_introspect__signature(qualname='maya.cmds.polySphere')",
        tb="Traceback (most recent call last):\n  ...",
    )
    assert "_meta" in result
    trace = result["_meta"]["dcc.raw_trace"]
    assert trace["underlying_call"] == "maya.cmds.polySphere(name='x', radius=-1.0)"
    assert trace["recipe_hint"] == "references/RECIPES.md#create_sphere"
    assert trace["introspect_hint"] == "dcc_introspect__signature(qualname='maya.cmds.polySphere')"
    assert "Traceback" in trace["traceback"]


def test_meta_absent_when_no_trace_fields():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace("Failed", "error string")
    assert "_meta" not in result


def test_underlying_call_truncated():
    from dcc_mcp_core import skill_error_with_trace

    long_call = "x" * 600
    result = skill_error_with_trace("msg", "err", underlying_call=long_call)
    assert len(result["_meta"]["dcc.raw_trace"]["underlying_call"]) == 500


def test_partial_trace_fields():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace("msg", "err", recipe_hint="RECIPES.md#foo")
    trace = result["_meta"]["dcc.raw_trace"]
    assert "recipe_hint" in trace
    assert "underlying_call" not in trace
    assert "traceback" not in trace


def test_context_kwargs_preserved():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace(
        "msg",
        "err",
        underlying_call="some.call()",
        extra_field="hello",
        count=42,
    )
    assert result["context"]["extra_field"] == "hello"
    assert result["context"]["count"] == 42


def test_possible_solutions_in_context():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace(
        "msg",
        "err",
        possible_solutions=["Try radius > 0", "Check Maya version"],
    )
    assert result["context"]["possible_solutions"] == ["Try radius > 0", "Check Maya version"]


def test_importable_from_top_level():
    import dcc_mcp_core

    assert hasattr(dcc_mcp_core, "skill_error_with_trace")
    assert callable(dcc_mcp_core.skill_error_with_trace)


def test_custom_prompt():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace("msg", "err", prompt="Custom recovery hint")
    assert result["prompt"] == "Custom recovery hint"


def test_default_prompt_present():
    from dcc_mcp_core import skill_error_with_trace

    result = skill_error_with_trace("msg", "err")
    assert result["prompt"]
