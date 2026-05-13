from __future__ import annotations

import pytest

import dcc_mcp_core
from dcc_mcp_core.host import normalize_tool_arguments
from dcc_mcp_core.host import normalize_tool_meta


def test_normalize_tool_arguments_accepts_python_dict() -> None:
    assert normalize_tool_arguments({"radius": 2, "name": "sphere"}) == {
        "radius": 2,
        "name": "sphere",
    }


def test_normalize_tool_arguments_accepts_json_string() -> None:
    assert normalize_tool_arguments('{"code": "print(1)"}') == {"code": "print(1)"}


def test_normalize_tool_arguments_defaults_to_empty_object() -> None:
    assert normalize_tool_arguments() == {}
    assert normalize_tool_arguments(None) == {}
    assert normalize_tool_arguments("  ") == {}


def test_normalize_tool_arguments_rejects_non_object_shapes() -> None:
    with pytest.raises(ValueError, match="arguments-not-object"):
        normalize_tool_arguments([1, 2, 3])

    with pytest.raises(ValueError, match="arguments-decoded-not-object"):
        normalize_tool_arguments("[1, 2, 3]")

    with pytest.raises(ValueError, match="arguments-string-not-json"):
        normalize_tool_arguments("not json")


def test_normalize_tool_meta_accepts_object_string_and_none() -> None:
    assert normalize_tool_meta({"progressToken": "job-1"}) == {"progressToken": "job-1"}
    assert normalize_tool_meta('{"dcc": {"async": true}}') == {"dcc": {"async": True}}
    assert normalize_tool_meta() is None
    assert normalize_tool_meta(None) is None
    assert normalize_tool_meta("  ") is None


def test_normalize_tool_meta_rejects_non_object_shapes() -> None:
    with pytest.raises(ValueError, match="arguments-not-object"):
        normalize_tool_meta(True)

    with pytest.raises(ValueError, match="arguments-decoded-not-object"):
        normalize_tool_meta("42")


def test_top_level_lazy_exports_host_wire_helpers() -> None:
    assert dcc_mcp_core.normalize_tool_arguments('{"x": 1}') == {"x": 1}
    assert dcc_mcp_core.normalize_tool_meta(None) is None
