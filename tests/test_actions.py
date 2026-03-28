"""Tests for ActionRegistry and EventBus."""

import dcc_mcp_core


class TestActionRegistry:
    def test_create(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert len(reg) == 0

    def test_register_and_get(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="create_sphere", description="Create a sphere", dcc="maya")
        assert len(reg) == 1
        meta = reg.get_action("create_sphere")
        assert meta is not None
        assert meta["name"] == "create_sphere"

    def test_get_action_by_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="action1", dcc="maya")
        reg.register(name="action2", dcc="blender")
        assert reg.get_action("action1", dcc_name="maya") is not None
        assert reg.get_action("action1", dcc_name="blender") is None

    def test_list_actions_for_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="maya")
        reg.register(name="a3", dcc="blender")
        names = reg.list_actions_for_dcc("maya")
        assert sorted(names) == ["a1", "a2"]

    def test_get_all_dccs(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        dccs = sorted(reg.get_all_dccs())
        assert dccs == ["blender", "maya"]

    def test_reset(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        assert len(reg) == 1
        reg.reset()
        assert len(reg) == 0

    def test_repr(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert "ActionRegistry" in repr(reg)


class TestEventBus:
    def test_create(self):
        bus = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(bus)
