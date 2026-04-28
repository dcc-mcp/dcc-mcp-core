"""Tests for ``MinimalModeConfig`` declarative progressive loading (issue #525)."""

# Import built-in modules
from __future__ import annotations

from typing import Any
from unittest.mock import MagicMock

# Import third-party modules
import pytest

# Import local modules
import dcc_mcp_core
from dcc_mcp_core._server.minimal_mode import MinimalModeConfig
from dcc_mcp_core._server.minimal_mode import apply_minimal_mode
from dcc_mcp_core._server.minimal_mode import resolve_default_tools
from dcc_mcp_core._server.minimal_mode import resolve_minimal_disabled

# ── public surface ───────────────────────────────────────────────────────────


def test_minimal_mode_config_is_exported() -> None:
    assert hasattr(dcc_mcp_core, "MinimalModeConfig")
    assert "MinimalModeConfig" in dcc_mcp_core.__all__
    assert dcc_mcp_core.MinimalModeConfig is MinimalModeConfig


def test_dataclass_is_frozen() -> None:
    # Import built-in modules
    from dataclasses import FrozenInstanceError

    cfg = MinimalModeConfig(skills=("a", "b"))
    with pytest.raises(FrozenInstanceError):
        cfg.skills = ("c",)  # type: ignore[misc]


def test_default_env_var_names() -> None:
    cfg = MinimalModeConfig(skills=())
    assert cfg.env_var_minimal == "DCC_MCP_MINIMAL"
    assert cfg.env_var_default_tools == "DCC_MCP_DEFAULT_TOOLS"


# ── env-var resolvers ────────────────────────────────────────────────────────


@pytest.mark.parametrize(
    ("value", "expected"),
    [
        (None, False),
        ("1", False),
        ("true", False),
        ("yes", False),
        ("on", False),
        ("0", True),
        ("false", True),
        ("False", True),
        ("NO", True),
        ("off", True),
        ("", True),
        ("  0  ", True),
    ],
)
def test_resolve_minimal_disabled(value: str | None, expected: bool) -> None:
    env = {} if value is None else {"DCC_MCP_MINIMAL": value}
    assert resolve_minimal_disabled("DCC_MCP_MINIMAL", env) is expected


def test_resolve_default_tools_unset_returns_none() -> None:
    assert resolve_default_tools("DCC_MCP_DEFAULT_TOOLS", {}) is None


def test_resolve_default_tools_empty_returns_none() -> None:
    assert resolve_default_tools("X", {"X": ""}) is None
    assert resolve_default_tools("X", {"X": "   ,, ,"}) is None


def test_resolve_default_tools_comma_separated() -> None:
    assert resolve_default_tools("X", {"X": "skill_a,skill_b"}) == ("skill_a", "skill_b")


def test_resolve_default_tools_whitespace_separated() -> None:
    assert resolve_default_tools("X", {"X": "  skill_a  skill_b "}) == ("skill_a", "skill_b")


def test_resolve_default_tools_mixed_separators() -> None:
    assert resolve_default_tools("X", {"X": "a, b ,c\nd"}) == ("a", "b", "c", "d")


def test_resolve_default_tools_dedupes_preserving_order() -> None:
    assert resolve_default_tools("X", {"X": "a, b, a, c, b"}) == ("a", "b", "c")


# ── apply_minimal_mode integration ───────────────────────────────────────────


class _FakeServer:
    """Minimal stand-in for the McpHttpServer surface that ``apply_minimal_mode`` touches."""

    def __init__(self, *, discovered: list[str] | None = None) -> None:
        self.loaded: list[str] = []
        self.deactivated: list[str] = []
        self._discovered = discovered or []
        self.catalog = MagicMock()
        self.catalog.deactivate_group = self._deactivate

    def _deactivate(self, group: str) -> None:
        self.deactivated.append(group)

    def load_skill(self, name: str) -> None:
        self.loaded.append(name)

    def list_skills(self) -> list[Any]:
        out: list[Any] = []
        for n in self._discovered:
            m = MagicMock()
            m.name = n  # MagicMock(name=…) is reserved for the mock's repr name
            out.append(m)
        return out


def test_apply_minimal_mode_default_path() -> None:
    server = _FakeServer()
    cfg = MinimalModeConfig(
        skills=("skill_a", "skill_b"),
        deactivate_groups={"skill_a": ("preview", "advanced")},
    )
    loaded = apply_minimal_mode(server, cfg, environ={})
    assert loaded == 2
    assert server.loaded == ["skill_a", "skill_b"]
    assert server.deactivated == ["preview", "advanced"]


def test_apply_minimal_mode_skips_deactivate_for_unloaded_skill() -> None:
    server = _FakeServer()
    cfg = MinimalModeConfig(
        skills=("skill_a",),
        deactivate_groups={"skill_b": ("preview",)},  # skill_b not loaded
    )
    apply_minimal_mode(server, cfg, environ={})
    assert server.deactivated == []


def test_explicit_override_via_default_tools_env() -> None:
    server = _FakeServer()
    cfg = MinimalModeConfig(skills=("skill_a",), deactivate_groups={"skill_a": ("preview",)})
    loaded = apply_minimal_mode(
        server,
        cfg,
        environ={"DCC_MCP_DEFAULT_TOOLS": "skill_x, skill_y"},
    )
    assert loaded == 2
    assert server.loaded == ["skill_x", "skill_y"]
    # deactivate_groups must NOT be applied when default-tools overrides
    assert server.deactivated == []


def test_minimal_disabled_loads_all_discovered_skills() -> None:
    server = _FakeServer(discovered=["a", "b", "c"])
    cfg = MinimalModeConfig(skills=("a",))
    loaded = apply_minimal_mode(server, cfg, environ={"DCC_MCP_MINIMAL": "0"})
    assert loaded == 3
    assert sorted(server.loaded) == ["a", "b", "c"]
    assert server.deactivated == []


def test_default_tools_takes_precedence_over_minimal_disabled() -> None:
    server = _FakeServer(discovered=["a", "b", "c"])
    cfg = MinimalModeConfig(skills=("z",))
    loaded = apply_minimal_mode(
        server,
        cfg,
        environ={"DCC_MCP_MINIMAL": "0", "DCC_MCP_DEFAULT_TOOLS": "x,y"},
    )
    assert loaded == 2
    assert server.loaded == ["x", "y"]


def test_load_failures_are_swallowed_and_counted() -> None:
    server = _FakeServer()
    server.load_skill = MagicMock(side_effect=[None, RuntimeError("boom"), None])  # type: ignore[assignment]
    cfg = MinimalModeConfig(skills=("a", "b", "c"))
    loaded = apply_minimal_mode(server, cfg, environ={})
    assert loaded == 2  # second skill failed


def test_missing_catalog_attribute_does_not_crash() -> None:
    server = _FakeServer()
    server.catalog = None  # type: ignore[assignment]
    cfg = MinimalModeConfig(
        skills=("a",),
        deactivate_groups={"a": ("group_x",)},
    )
    loaded = apply_minimal_mode(server, cfg, environ={})
    assert loaded == 1  # skill loaded, deactivation silently skipped


def test_custom_env_var_names() -> None:
    server = _FakeServer(discovered=["disco_a"])
    cfg = MinimalModeConfig(
        skills=("default_a",),
        env_var_minimal="MAYA_MINIMAL",
        env_var_default_tools="MAYA_TOOLS",
    )
    apply_minimal_mode(server, cfg, environ={"MAYA_TOOLS": "explicit_a"})
    assert server.loaded == ["explicit_a"]
