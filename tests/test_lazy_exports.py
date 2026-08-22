"""Regression tests for the dependency-free lazy export resolver."""

from __future__ import annotations

import sys
from types import ModuleType

import pytest

from dcc_mcp_core._lazy import resolve_lazy_symbol


def test_legitimate_none_export_is_not_treated_as_missing(monkeypatch: pytest.MonkeyPatch) -> None:
    source_name = "dcc_mcp_core_test_none_source"
    caller_name = "dcc_mcp_core_test_none_caller"
    source = ModuleType(source_name)
    source.value = None
    caller = ModuleType(caller_name)
    monkeypatch.setitem(sys.modules, source_name, source)
    monkeypatch.setitem(sys.modules, caller_name, caller)

    assert resolve_lazy_symbol("value", {"value": source_name}, module_name=caller_name) is None
    assert caller.value is None


def test_missing_non_optional_export_still_raises(monkeypatch: pytest.MonkeyPatch) -> None:
    source_name = "dcc_mcp_core_test_missing_source"
    caller_name = "dcc_mcp_core_test_missing_caller"
    monkeypatch.setitem(sys.modules, source_name, ModuleType(source_name))
    monkeypatch.setitem(sys.modules, caller_name, ModuleType(caller_name))

    with pytest.raises(AttributeError, match="has no attribute 'value'"):
        resolve_lazy_symbol("value", {"value": source_name}, module_name=caller_name)
