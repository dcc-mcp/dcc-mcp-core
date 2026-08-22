from __future__ import annotations

import pytest

import dcc_mcp_core
from dcc_mcp_core import _json_codec
from dcc_mcp_core import wire as native_wire
from dcc_mcp_core.wire import _fallback as lite_wire
from dcc_mcp_core.wire import normalize_tool_arguments
from dcc_mcp_core.wire import normalize_tool_meta


@pytest.fixture(params=("native", "lite"))
def wire_backend(request, monkeypatch):
    """Return the native or py37-lite implementation under one contract."""
    if request.param == "native":
        return native_wire
    monkeypatch.setattr(_json_codec, "_optional_core_symbol", lambda _name: None)
    return lite_wire


@pytest.mark.parametrize(
    ("value", "expected"),
    (
        (None, {}),
        ("", {}),
        ({"radius": 2}, {"radius": 2}),
        ('{"radius": 2}', {"radius": 2}),
    ),
)
def test_wire_backends_share_arguments_contract(wire_backend, value, expected) -> None:
    assert wire_backend.normalize_tool_arguments(value) == expected


@pytest.mark.parametrize(
    ("value", "expected"),
    (
        (None, None),
        ("", None),
        ({"progressToken": "job-1"}, {"progressToken": "job-1"}),
        ('{"progressToken": "job-1"}', {"progressToken": "job-1"}),
    ),
)
def test_wire_backends_share_meta_contract(wire_backend, value, expected) -> None:
    assert wire_backend.normalize_tool_meta(value) == expected


@pytest.mark.parametrize(
    "value",
    (
        "not json",
        [],
        42,
        True,
        "[]",
        "42",
        "true",
    ),
)
def test_wire_backends_reject_invalid_and_non_object_roots(wire_backend, value) -> None:
    with pytest.raises(ValueError):
        wire_backend.normalize_tool_arguments(value)
    with pytest.raises(ValueError):
        wire_backend.normalize_tool_meta(value)


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


def test_top_level_lazy_exports_wire_helpers() -> None:
    assert dcc_mcp_core.normalize_tool_arguments('{"x": 1}') == {"x": 1}
    assert dcc_mcp_core.normalize_tool_meta(None) is None


def test_host_keeps_backward_compatible_wire_aliases() -> None:
    from dcc_mcp_core.host import normalize_tool_arguments as host_normalize_arguments
    from dcc_mcp_core.host import normalize_tool_meta as host_normalize_meta

    assert host_normalize_arguments('{"legacy": true}') == {"legacy": True}
    assert host_normalize_meta(None) is None
