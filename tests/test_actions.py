"""Tests for ActionRegistry and EventBus."""

# Import local modules
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
        assert meta["description"] == "Create a sphere"
        assert meta["dcc"] == "maya"

    def test_register_with_all_params(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(
            name="my_action",
            description="Do something",
            category="tools",
            tags=["geo", "create"],
            dcc="houdini",
            version="2.0.0",
            input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}',
            output_schema='{"type": "object"}',
            source_file="/path/to/action.py",
        )
        meta = reg.get_action("my_action")
        assert meta["category"] == "tools"
        assert meta["tags"] == ["geo", "create"]
        assert meta["version"] == "2.0.0"
        assert meta["source_file"] == "/path/to/action.py"
        assert "radius" in meta["input_schema"]

    def test_register_defaults(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="default_action")
        meta = reg.get_action("default_action")
        assert meta["dcc"] == "python"
        assert meta["version"] == "1.0.0"
        assert meta["category"] == ""
        assert meta["tags"] == []
        assert meta["source_file"] is None

    def test_get_action_none(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_action("nonexistent") is None

    def test_get_action_by_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="action1", dcc="maya")
        reg.register(name="action2", dcc="blender")
        assert reg.get_action("action1", dcc_name="maya") is not None
        assert reg.get_action("action1", dcc_name="blender") is None

    def test_get_action_nonexistent_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        assert reg.get_action("a1", dcc_name="nonexistent") is None

    def test_list_actions_for_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="maya")
        reg.register(name="a3", dcc="blender")
        names = reg.list_actions_for_dcc("maya")
        assert sorted(names) == ["a1", "a2"]

    def test_list_actions_for_dcc_empty(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.list_actions_for_dcc("nonexistent") == []

    def test_list_actions_all(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        actions = reg.list_actions()
        assert len(actions) == 2
        names = {a["name"] for a in actions}
        assert names == {"a1", "a2"}

    def test_list_actions_filtered_by_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        actions = reg.list_actions(dcc_name="maya")
        assert len(actions) == 1
        assert actions[0]["name"] == "a1"

    def test_list_actions_empty_dcc(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.list_actions(dcc_name="nonexistent") == []

    def test_get_all_dccs(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        dccs = sorted(reg.get_all_dccs())
        assert dccs == ["blender", "maya"]

    def test_get_all_dccs_empty(self):
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_all_dccs() == []

    def test_reset(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        assert len(reg) == 1
        reg.reset()
        assert len(reg) == 0
        assert reg.get_all_dccs() == []

    def test_overwrite_action(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", description="v1", dcc="maya")
        reg.register(name="a1", description="v2", dcc="maya")
        meta = reg.get_action("a1")
        assert meta["description"] == "v2"

    def test_repr(self):
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1")
        assert "ActionRegistry" in repr(reg)
        assert "1" in repr(reg)


class TestEventBus:
    def test_create(self):
        bus = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(bus)
        assert "0" in repr(bus)

    def test_subscribe_and_publish(self):
        bus = dcc_mcp_core.EventBus()
        results = []
        sub_id = bus.subscribe("test_event", lambda: results.append("called"))
        assert isinstance(sub_id, int)
        assert sub_id > 0
        bus.publish("test_event")
        assert results == ["called"]

    def test_subscribe_multiple(self):
        bus = dcc_mcp_core.EventBus()
        results = []
        bus.subscribe("evt", lambda: results.append("a"))
        bus.subscribe("evt", lambda: results.append("b"))
        bus.publish("evt")
        assert sorted(results) == ["a", "b"]

    def test_publish_with_kwargs(self):
        bus = dcc_mcp_core.EventBus()
        results = []
        bus.subscribe("evt", lambda **kw: results.append(kw))
        bus.publish("evt", x=1, y="hello")
        assert results[0] == {"x": 1, "y": "hello"}

    def test_publish_no_subscribers(self):
        bus = dcc_mcp_core.EventBus()
        bus.publish("nonexistent")  # should not error

    def test_unsubscribe(self):
        bus = dcc_mcp_core.EventBus()
        results = []
        sub_id = bus.subscribe("evt", lambda: results.append("x"))
        removed = bus.unsubscribe("evt", sub_id)
        assert removed is True
        bus.publish("evt")
        assert results == []

    def test_unsubscribe_nonexistent_id(self):
        bus = dcc_mcp_core.EventBus()
        bus.subscribe("evt", lambda: None)
        removed = bus.unsubscribe("evt", 9999)
        assert removed is False

    def test_unsubscribe_nonexistent_event(self):
        bus = dcc_mcp_core.EventBus()
        removed = bus.unsubscribe("nonexistent", 1)
        assert removed is False

    def test_subscribe_returns_unique_ids(self):
        bus = dcc_mcp_core.EventBus()
        id1 = bus.subscribe("a", lambda: None)
        id2 = bus.subscribe("b", lambda: None)
        id3 = bus.subscribe("a", lambda: None)
        assert id1 != id2 != id3
