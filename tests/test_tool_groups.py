"""Tests for progressive tool exposure (``SkillGroup`` / ``activate_tool_group``).

Covers:
- ``SkillGroup`` construction and field access.
- ``ToolRegistry`` group helpers (``set_group_enabled``, ``list_groups``,
  ``list_actions_enabled``, ``list_actions_in_group``).
- ``SkillCatalog`` activation methods.
- Dispatcher refusal on disabled actions.
"""

# Import future modules
from __future__ import annotations

# Import third-party modules
import pytest

# Import local modules
from dcc_mcp_core import SkillGroup
from dcc_mcp_core import ToolDispatcher
from dcc_mcp_core import ToolRegistry

# ── SkillGroup ───────────────────────────────────────────────────────────────


class TestSkillGroup:
    def test_construction(self) -> None:
        g = SkillGroup(name="uv-editing", description="UV ops", tools=["unwrap", "layout"])
        assert g.name == "uv-editing"
        assert g.description == "UV ops"
        assert g.tools == ["unwrap", "layout"]
        assert g.default_active is False

    def test_default_active_flag(self) -> None:
        g = SkillGroup(name="modeling", default_active=True)
        assert g.default_active is True

    def test_repr(self) -> None:
        g = SkillGroup(name="x", tools=["a", "b", "c"])
        r = repr(g)
        assert "SkillGroup" in r
        assert 'name="x"' in r or "name='x'" in r or '"x"' in r


# ── ToolRegistry group helpers ───────────────────────────────────────────────


@pytest.fixture
def registry_with_groups() -> ToolRegistry:
    r = ToolRegistry()
    r.register(name="mod_a", description="A", dcc="maya", group="modeling", enabled=True)
    r.register(name="mod_b", description="B", dcc="maya", group="modeling", enabled=True)
    r.register(name="rig_a", description="A", dcc="maya", group="rigging", enabled=False)
    r.register(name="rig_b", description="B", dcc="maya", group="rigging", enabled=False)
    r.register(name="free_tool", description="no group", dcc="maya")
    return r


class TestToolRegistryGroups:
    def test_list_groups(self, registry_with_groups) -> None:
        groups = set(registry_with_groups.list_groups())
        assert groups == {"modeling", "rigging"}

    def test_list_actions_in_group(self, registry_with_groups) -> None:
        rigging = {m["name"] for m in registry_with_groups.list_actions_in_group("rigging")}
        assert rigging == {"rig_a", "rig_b"}

    def test_list_actions_enabled_excludes_disabled(self, registry_with_groups) -> None:
        enabled = {m["name"] for m in registry_with_groups.list_actions_enabled()}
        assert enabled == {"mod_a", "mod_b", "free_tool"}

    def test_set_group_enabled_activates(self, registry_with_groups) -> None:
        changed = registry_with_groups.set_group_enabled("rigging", True)
        assert changed == 2
        enabled = {m["name"] for m in registry_with_groups.list_actions_enabled()}
        assert {"rig_a", "rig_b"} <= enabled

    def test_set_group_enabled_deactivates(self, registry_with_groups) -> None:
        registry_with_groups.set_group_enabled("modeling", False)
        enabled = {m["name"] for m in registry_with_groups.list_actions_enabled()}
        assert enabled == {"free_tool"}

    def test_set_action_enabled_single(self, registry_with_groups) -> None:
        assert registry_with_groups.set_action_enabled("rig_a", True) is True
        enabled = {m["name"] for m in registry_with_groups.list_actions_enabled()}
        assert "rig_a" in enabled
        assert "rig_b" not in enabled

    def test_set_action_enabled_returns_false_for_unknown(self, registry_with_groups) -> None:
        assert registry_with_groups.set_action_enabled("does_not_exist", True) is False

    def test_group_field_preserved_in_list(self, registry_with_groups) -> None:
        metas = {m["name"]: m for m in registry_with_groups.list_actions()}
        assert metas["rig_a"]["group"] == "rigging"
        assert metas["rig_a"]["enabled"] is False
        assert metas["free_tool"]["group"] == ""
        assert metas["free_tool"]["enabled"] is True


# ── Dispatcher enforcement ───────────────────────────────────────────────────


class TestDispatcherEnforcesEnabled:
    def test_disabled_action_raises(self, registry_with_groups) -> None:
        dispatcher = ToolDispatcher(registry_with_groups)
        dispatcher.register_handler("rig_a", lambda _params: {"ok": True})
        # Disabled action must be refused; the Rust DispatchError::ActionDisabled
        # variant surfaces as PermissionError in Python.
        with pytest.raises(PermissionError):
            dispatcher.dispatch("rig_a", "{}")

    def test_enabled_action_dispatches(self, registry_with_groups) -> None:
        dispatcher = ToolDispatcher(registry_with_groups)
        dispatcher.register_handler("mod_a", lambda _params: {"ok": True})
        result = dispatcher.dispatch("mod_a", "{}")
        assert result["action"] == "mod_a"

    def test_reactivated_action_dispatches(self, registry_with_groups) -> None:
        dispatcher = ToolDispatcher(registry_with_groups)
        dispatcher.register_handler("rig_a", lambda _params: {"ok": True})
        registry_with_groups.set_group_enabled("rigging", True)
        result = dispatcher.dispatch("rig_a", "{}")
        assert result["action"] == "rig_a"
