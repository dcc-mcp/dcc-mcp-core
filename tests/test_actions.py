"""Tests for ActionRegistry and EventBus."""

# Import future modules
from __future__ import annotations

# Import local modules
import dcc_mcp_core


class TestActionRegistry:
    def test_create(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert len(reg) == 0

    def test_register_and_get(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="create_sphere", description="Create a sphere", dcc="maya")
        assert len(reg) == 1
        meta = reg.get_action("create_sphere")
        assert meta is not None
        assert meta["name"] == "create_sphere"
        assert meta["description"] == "Create a sphere"
        assert meta["dcc"] == "maya"

    def test_register_with_all_params(self) -> None:
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
        assert "radius" in meta["input_schema"]["properties"]

    def test_register_defaults(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="default_action")
        meta = reg.get_action("default_action")
        assert meta["dcc"] == "python"
        assert meta["version"] == "1.0.0"
        assert meta["category"] == ""
        assert meta["tags"] == []
        assert meta["source_file"] is None

    def test_get_action_none(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_action("nonexistent") is None

    def test_get_action_by_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="action1", dcc="maya")
        reg.register(name="action2", dcc="blender")
        assert reg.get_action("action1", dcc_name="maya") is not None
        assert reg.get_action("action1", dcc_name="blender") is None

    def test_get_action_nonexistent_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        assert reg.get_action("a1", dcc_name="nonexistent") is None

    def test_list_actions_for_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="maya")
        reg.register(name="a3", dcc="blender")
        names = reg.list_actions_for_dcc("maya")
        assert sorted(names) == ["a1", "a2"]

    def test_list_actions_for_dcc_empty(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.list_actions_for_dcc("nonexistent") == []

    def test_list_actions_all(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        actions = reg.list_actions()
        assert len(actions) == 2
        names = {a["name"] for a in actions}
        assert names == {"a1", "a2"}

    def test_list_actions_filtered_by_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        actions = reg.list_actions(dcc_name="maya")
        assert len(actions) == 1
        assert actions[0]["name"] == "a1"

    def test_list_actions_empty_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.list_actions(dcc_name="nonexistent") == []

    def test_get_all_dccs(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        dccs = sorted(reg.get_all_dccs())
        assert dccs == ["blender", "maya"]

    def test_get_all_dccs_empty(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_all_dccs() == []

    def test_reset(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        assert len(reg) == 1
        reg.reset()
        assert len(reg) == 0
        assert reg.get_all_dccs() == []

    def test_overwrite_action(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", description="v1", dcc="maya")
        reg.register(name="a1", description="v2", dcc="maya")
        meta = reg.get_action("a1")
        assert meta["description"] == "v2"

    def test_repr(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1")
        assert "ActionRegistry" in repr(reg)
        assert "1" in repr(reg)

    # ── search_actions ─────────────────────────────────────────────────────────

    def test_search_actions_by_category(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="create_sphere", category="geometry", dcc="maya")
        reg.register(name="delete_sphere", category="geometry", dcc="maya")
        reg.register(name="export_fbx", category="export", dcc="maya")
        results = reg.search_actions(category="geometry")
        assert len(results) == 2
        names = {r["name"] for r in results}
        assert names == {"create_sphere", "delete_sphere"}

    def test_search_actions_by_tag(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="create_sphere", tags=["create", "geo"], dcc="maya")
        reg.register(name="delete_sphere", tags=["delete", "geo"], dcc="maya")
        reg.register(name="export_fbx", tags=["export"], dcc="maya")
        results = reg.search_actions(tags=["geo"])
        assert len(results) == 2

    def test_search_actions_by_multiple_tags(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", tags=["create", "geo", "primitive"], dcc="maya")
        reg.register(name="a2", tags=["create", "geo"], dcc="maya")
        reg.register(name="a3", tags=["geo"], dcc="maya")
        # AND filter: must have all tags
        results = reg.search_actions(tags=["create", "primitive"])
        assert len(results) == 1
        assert results[0]["name"] == "a1"

    def test_search_actions_by_category_and_tag(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", tags=["create"], dcc="maya")
        reg.register(name="a2", category="geometry", tags=["delete"], dcc="maya")
        reg.register(name="a3", category="export", tags=["create"], dcc="maya")
        results = reg.search_actions(category="geometry", tags=["create"])
        assert len(results) == 1
        assert results[0]["name"] == "a1"

    def test_search_actions_by_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        reg.register(name="a2", category="geometry", dcc="blender")
        results = reg.search_actions(category="geometry", dcc_name="maya")
        assert len(results) == 1
        assert results[0]["name"] == "a1"

    def test_search_actions_no_filter_returns_all(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="blender")
        results = reg.search_actions()
        assert len(results) == 2

    def test_search_actions_empty_category_returns_all(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        reg.register(name="a2", category="export", dcc="maya")
        # Empty string category should not filter
        results = reg.search_actions(category="")
        assert len(results) == 2

    def test_search_actions_no_match_returns_empty(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        results = reg.search_actions(category="nonexistent")
        assert len(results) == 0

    def test_search_actions_empty_registry(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.search_actions(category="geometry") == []

    # ── get_categories ─────────────────────────────────────────────────────────

    def test_get_categories_sorted_dedup(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        reg.register(name="a2", category="geometry", dcc="maya")
        reg.register(name="a3", category="export", dcc="maya")
        cats = reg.get_categories()
        assert cats == ["export", "geometry"]

    def test_get_categories_scoped_to_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        reg.register(name="a2", category="render", dcc="blender")
        assert reg.get_categories(dcc_name="maya") == ["geometry"]
        assert reg.get_categories(dcc_name="blender") == ["render"]

    def test_get_categories_skips_empty_category(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1")  # no category → default ""
        reg.register(name="a2", category="tools")
        cats = reg.get_categories()
        assert cats == ["tools"]  # empty strings excluded

    def test_get_categories_empty_registry(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_categories() == []

    # ── get_tags ───────────────────────────────────────────────────────────────

    def test_get_tags_sorted_dedup(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", tags=["geo", "create"], dcc="maya")
        reg.register(name="a2", tags=["geo", "delete"], dcc="maya")
        tags = reg.get_tags()
        assert tags == ["create", "delete", "geo"]

    def test_get_tags_scoped_to_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", tags=["maya_tag"], dcc="maya")
        reg.register(name="a2", tags=["blender_tag"], dcc="blender")
        assert reg.get_tags(dcc_name="maya") == ["maya_tag"]
        assert reg.get_tags(dcc_name="blender") == ["blender_tag"]

    def test_get_tags_empty_registry(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.get_tags() == []

    def test_get_tags_no_tags(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1")  # no tags
        assert reg.get_tags() == []

    # ── count_actions ──────────────────────────────────────────────────────────

    def test_count_actions_all(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", dcc="maya")
        reg.register(name="a2", dcc="maya")
        assert reg.count_actions() == 2

    def test_count_actions_by_category(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        reg.register(name="a2", category="export", dcc="maya")
        assert reg.count_actions(category="geometry") == 1
        assert reg.count_actions(category="export") == 1

    def test_count_actions_no_match(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", dcc="maya")
        assert reg.count_actions(category="nonexistent") == 0

    def test_count_actions_matches_search_results_len(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="a1", category="geometry", tags=["create"], dcc="maya")
        reg.register(name="a2", category="geometry", tags=["delete"], dcc="maya")
        search_len = len(reg.search_actions(category="geometry", tags=["create"]))
        count = reg.count_actions(category="geometry", tags=["create"])
        assert search_len == count


class TestRegisterBatch:
    def test_empty_list_is_noop(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch([])
        assert len(reg) == 0

    def test_inserts_all_actions(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch(
            [
                {"name": "op_a", "dcc": "maya"},
                {"name": "op_b", "dcc": "maya"},
                {"name": "op_c", "dcc": "blender"},
            ]
        )
        assert len(reg) == 3
        assert reg.get_action("op_a") is not None
        assert reg.get_action("op_b") is not None
        assert reg.get_action("op_c") is not None

    def test_respects_dcc_scope(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch(
            [
                {"name": "op1", "dcc": "maya"},
                {"name": "op2", "dcc": "blender"},
                {"name": "op3", "dcc": "maya"},
            ]
        )
        maya_actions = reg.list_actions_for_dcc("maya")
        blender_actions = reg.list_actions_for_dcc("blender")
        assert len(maya_actions) == 2
        assert len(blender_actions) == 1

    def test_fields_from_dict(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch(
            [
                {
                    "name": "create_sphere",
                    "description": "Makes a sphere",
                    "category": "geometry",
                    "tags": ["create", "mesh"],
                    "dcc": "maya",
                    "version": "2.0.0",
                }
            ]
        )
        meta = reg.get_action("create_sphere")
        assert meta is not None
        assert meta["description"] == "Makes a sphere"
        assert meta["category"] == "geometry"
        assert meta["tags"] == ["create", "mesh"]
        assert meta["version"] == "2.0.0"

    def test_skip_entries_without_name(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch(
            [
                {"description": "no name here"},
                {"name": "", "dcc": "maya"},
                {"name": "valid", "dcc": "maya"},
            ]
        )
        assert len(reg) == 1
        assert reg.get_action("valid") is not None

    def test_overwrites_existing_action(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="dup", description="original", dcc="maya")
        reg.register_batch([{"name": "dup", "description": "replaced", "dcc": "maya"}])
        meta = reg.get_action("dup")
        assert meta is not None
        assert meta["description"] == "replaced"
        assert len(reg) == 1

    def test_defaults_for_missing_fields(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register_batch([{"name": "minimal"}])
        meta = reg.get_action("minimal")
        assert meta is not None
        assert meta["category"] == ""
        assert meta["tags"] == []

    def test_non_dict_entries_are_skipped(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        # Only dicts are valid — other types should be silently skipped.
        reg.register_batch(
            [
                "not_a_dict",
                42,
                None,
                {"name": "valid_only", "dcc": "maya"},
            ]
        )
        assert len(reg) == 1

    def test_combined_with_register(self) -> None:
        """register_batch and register can be freely mixed."""
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="single", dcc="maya")
        reg.register_batch(
            [
                {"name": "batch_a", "dcc": "maya"},
                {"name": "batch_b", "dcc": "maya"},
            ]
        )
        assert len(reg) == 3


class TestUnregister:
    def test_returns_true_when_found(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="to_remove", dcc="maya")
        assert reg.unregister("to_remove") is True

    def test_returns_false_when_not_found(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        assert reg.unregister("nonexistent") is False

    def test_removes_from_global_registry(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="gone", dcc="maya")
        reg.unregister("gone")
        assert reg.get_action("gone") is None
        assert len(reg) == 0

    def test_removes_from_dcc_map(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="gone", dcc="maya")
        reg.unregister("gone")
        assert reg.get_action("gone", dcc_name="maya") is None
        assert reg.list_actions_for_dcc("maya") == []

    def test_global_removes_from_all_dccs(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="shared", dcc="maya")
        reg.register(name="shared", dcc="blender")
        reg.unregister("shared")
        assert reg.get_action("shared", dcc_name="maya") is None
        assert reg.get_action("shared", dcc_name="blender") is None
        assert len(reg) == 0

    def test_scoped_removes_only_target_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="op", dcc="maya")
        reg.register(name="op", dcc="blender")
        result = reg.unregister("op", dcc_name="maya")
        assert result is True
        # Blender entry must survive.
        assert reg.get_action("op", dcc_name="blender") is not None

    def test_scoped_clears_global_when_last_dcc(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="only_maya", dcc="maya")
        reg.unregister("only_maya", dcc_name="maya")
        assert reg.get_action("only_maya") is None
        assert len(reg) == 0

    def test_scoped_nonexistent_dcc_returns_false(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="op", dcc="maya")
        assert reg.unregister("op", dcc_name="blender") is False
        # Original entry must still be there.
        assert reg.get_action("op") is not None

    def test_idempotent_second_call_returns_false(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="once", dcc="maya")
        assert reg.unregister("once") is True
        assert reg.unregister("once") is False

    def test_unregister_one_does_not_affect_others(self) -> None:
        reg = dcc_mcp_core.ActionRegistry()
        reg.register(name="keep_me", dcc="maya")
        reg.register(name="remove_me", dcc="maya")
        reg.unregister("remove_me")
        assert reg.get_action("keep_me") is not None
        assert len(reg) == 1


class TestEventBus:
    def test_create(self) -> None:
        bus = dcc_mcp_core.EventBus()
        assert "EventBus" in repr(bus)
        assert "0" in repr(bus)

    def test_subscribe_and_publish(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        sub_id = bus.subscribe("test_event", lambda: results.append("called"))
        assert isinstance(sub_id, int)
        assert sub_id > 0
        bus.publish("test_event")
        assert results == ["called"]

    def test_subscribe_multiple(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        bus.subscribe("evt", lambda: results.append("a"))
        bus.subscribe("evt", lambda: results.append("b"))
        bus.publish("evt")
        assert sorted(results) == ["a", "b"]

    def test_publish_with_kwargs(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        bus.subscribe("evt", lambda **kw: results.append(kw))
        bus.publish("evt", x=1, y="hello")
        assert results[0] == {"x": 1, "y": "hello"}

    def test_publish_no_subscribers(self) -> None:
        bus = dcc_mcp_core.EventBus()
        bus.publish("nonexistent")  # should not error

    def test_unsubscribe(self) -> None:
        bus = dcc_mcp_core.EventBus()
        results = []
        sub_id = bus.subscribe("evt", lambda: results.append("x"))
        removed = bus.unsubscribe("evt", sub_id)
        assert removed is True
        bus.publish("evt")
        assert results == []

    def test_unsubscribe_nonexistent_id(self) -> None:
        bus = dcc_mcp_core.EventBus()
        bus.subscribe("evt", lambda: None)
        removed = bus.unsubscribe("evt", 9999)
        assert removed is False

    def test_unsubscribe_nonexistent_event(self) -> None:
        bus = dcc_mcp_core.EventBus()
        removed = bus.unsubscribe("nonexistent", 1)
        assert removed is False

    def test_subscribe_returns_unique_ids(self) -> None:
        bus = dcc_mcp_core.EventBus()
        id1 = bus.subscribe("a", lambda: None)
        id2 = bus.subscribe("b", lambda: None)
        id3 = bus.subscribe("a", lambda: None)
        assert len({id1, id2, id3}) == 3
