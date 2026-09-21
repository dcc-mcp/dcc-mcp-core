"""Regression tests for the dependency-free lazy export resolver."""

from __future__ import annotations

import sys
from types import ModuleType

import pytest

import dcc_mcp_core
from dcc_mcp_core._exports import _LAZY
from dcc_mcp_core._exports import _STABLE_LAZY
from dcc_mcp_core._exports import PUBLIC_EXPORTS
from dcc_mcp_core._lazy import resolve_lazy_symbol
from dcc_mcp_core.escape_hatch_policy import DEFAULT_PROMOTION_THRESHOLD as ESCAPE_HATCH_PROMOTION_THRESHOLD


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


# ``_ALL_LAZY`` in ``dcc_mcp_core._exports`` is a plain dict literal, so a second
# entry for a name silently overwrites the first one instead of raising. That is
# how the duplicate ``DEFAULT_PROMOTION_THRESHOLD`` key moved the top-level name
# off ``skill_promotion`` without any test noticing. Pin the source module here so
# re-pointing the export stays a deliberate, reviewable decision instead of a side
# effect of dict ordering.
PROMOTION_THRESHOLD_EXPORT_SOURCE = "dcc_mcp_core.escape_hatch_policy"


def test_default_promotion_threshold_resolves_from_the_pinned_module() -> None:
    assert _STABLE_LAZY["DEFAULT_PROMOTION_THRESHOLD"] == PROMOTION_THRESHOLD_EXPORT_SOURCE
    assert _LAZY["DEFAULT_PROMOTION_THRESHOLD"] == PROMOTION_THRESHOLD_EXPORT_SOURCE
    resolved_module = sys.modules[PROMOTION_THRESHOLD_EXPORT_SOURCE]
    assert resolved_module.DEFAULT_PROMOTION_THRESHOLD == dcc_mcp_core.DEFAULT_PROMOTION_THRESHOLD
    assert dcc_mcp_core.DEFAULT_PROMOTION_THRESHOLD == ESCAPE_HATCH_PROMOTION_THRESHOLD


def test_default_promotion_threshold_stays_a_public_export() -> None:
    assert "DEFAULT_PROMOTION_THRESHOLD" in PUBLIC_EXPORTS
    assert "DEFAULT_PROMOTION_THRESHOLD" in dcc_mcp_core.__all__
