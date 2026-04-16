"""Deep tests for ToolRegistry.count_actions, get_categories, and get_tags.

Covers multi-DCC multi-category scenarios, tag filtering, and combined filters.
"""

from __future__ import annotations

import pytest

import dcc_mcp_core


def _populate_registry() -> dcc_mcp_core.ToolRegistry:
    """Create a registry with actions across multiple DCCs and categories."""
    reg = dcc_mcp_core.ToolRegistry()
    reg.register_batch(
        [
            # maya - geo
            {"name": "create_sphere", "category": "geo", "dcc": "maya", "tags": ["create", "mesh"]},
            {"name": "create_cube", "category": "geo", "dcc": "maya", "tags": ["create", "mesh"]},
            {"name": "delete_mesh", "category": "geo", "dcc": "maya", "tags": ["delete", "mesh"]},
            # maya - render
            {"name": "render_scene", "category": "render", "dcc": "maya", "tags": ["render", "gpu"]},
            {"name": "render_hq", "category": "render", "dcc": "maya", "tags": ["render", "gpu", "hd"]},
            # blender - geo
            {"name": "add_primitive", "category": "geo", "dcc": "blender", "tags": ["create", "mesh"]},
            # blender - anim
            {"name": "bake_anim", "category": "anim", "dcc": "blender", "tags": ["bake", "anim"]},
            {"name": "set_keyframe", "category": "anim", "dcc": "blender", "tags": ["anim"]},
            # houdini - sim
            {"name": "cook_sim", "category": "sim", "dcc": "houdini", "tags": ["sim", "hpc"]},
        ]
    )
    return reg


class TestCountActionsBasic:
    def test_count_empty(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        assert reg.count_actions() == 0

    def test_count_single(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("my_action", description="d", category="c", dcc="maya")
        assert reg.count_actions() == 1

    def test_count_total(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions() == 9

    def test_count_by_dcc_maya(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(dcc_name="maya") == 5

    def test_count_by_dcc_blender(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(dcc_name="blender") == 3

    def test_count_by_dcc_houdini(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(dcc_name="houdini") == 1

    def test_count_by_dcc_unknown_zero(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(dcc_name="3dsmax") == 0

    def test_count_by_category_geo(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="geo") == 4

    def test_count_by_category_render(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="render") == 2

    def test_count_by_category_anim(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="anim") == 2

    def test_count_by_category_sim(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="sim") == 1

    def test_count_by_category_unknown_zero(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="nonexistent") == 0

    def test_count_by_single_tag(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(tags=["mesh"]) == 4

    def test_count_by_single_tag_gpu(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(tags=["gpu"]) == 2

    def test_count_by_single_tag_hd(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(tags=["hd"]) == 1

    def test_count_by_multi_tags_and_logic(self) -> None:
        reg = _populate_registry()
        # both "gpu" and "hd" -> only render_hq
        assert reg.count_actions(tags=["gpu", "hd"]) == 1

    def test_count_by_tag_create(self) -> None:
        reg = _populate_registry()
        # create_sphere, create_cube, add_primitive
        assert reg.count_actions(tags=["create"]) == 3


class TestCountActionsCombinedFilters:
    def test_category_and_dcc(self) -> None:
        reg = _populate_registry()
        # maya geo = create_sphere, create_cube, delete_mesh
        assert reg.count_actions(category="geo", dcc_name="maya") == 3

    def test_category_and_dcc_blender_geo(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="geo", dcc_name="blender") == 1

    def test_category_and_dcc_no_match(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(category="render", dcc_name="houdini") == 0

    def test_tag_and_dcc(self) -> None:
        reg = _populate_registry()
        # mesh in maya = create_sphere, create_cube, delete_mesh = 3
        assert reg.count_actions(tags=["mesh"], dcc_name="maya") == 3

    def test_tag_and_dcc_blender_mesh(self) -> None:
        reg = _populate_registry()
        assert reg.count_actions(tags=["mesh"], dcc_name="blender") == 1

    def test_category_tag_dcc(self) -> None:
        reg = _populate_registry()
        # geo + create + maya = create_sphere, create_cube
        assert reg.count_actions(category="geo", tags=["create"], dcc_name="maya") == 2

    def test_after_unregister(self) -> None:
        reg = _populate_registry()
        before = reg.count_actions(dcc_name="maya")
        reg.unregister("create_sphere", dcc_name="maya")
        assert reg.count_actions(dcc_name="maya") == before - 1

    def test_after_reset(self) -> None:
        reg = _populate_registry()
        reg.reset()
        assert reg.count_actions() == 0


class TestGetCategories:
    def test_empty_returns_empty(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        assert reg.get_categories() == []

    def test_categories_sorted(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("c", description="d", category="render", dcc="maya")
        reg.register("a", description="d", category="anim", dcc="maya")
        reg.register("g", description="d", category="geo", dcc="maya")
        cats = reg.get_categories()
        assert cats == sorted(cats)

    def test_categories_unique(self) -> None:
        reg = _populate_registry()
        cats = reg.get_categories()
        assert len(cats) == len(set(cats))

    def test_categories_all_dccs(self) -> None:
        reg = _populate_registry()
        cats = reg.get_categories()
        assert set(cats) == {"geo", "render", "anim", "sim"}

    def test_categories_for_specific_dcc_maya(self) -> None:
        reg = _populate_registry()
        cats = reg.get_categories(dcc_name="maya")
        assert set(cats) == {"geo", "render"}

    def test_categories_for_specific_dcc_blender(self) -> None:
        reg = _populate_registry()
        cats = reg.get_categories(dcc_name="blender")
        assert set(cats) == {"geo", "anim"}

    def test_categories_for_specific_dcc_houdini(self) -> None:
        reg = _populate_registry()
        cats = reg.get_categories(dcc_name="houdini")
        assert cats == ["sim"]

    def test_categories_unknown_dcc_empty(self) -> None:
        reg = _populate_registry()
        assert reg.get_categories(dcc_name="cinema4d") == []

    def test_categories_after_unregister_shrinks(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("cook", description="d", category="sim", dcc="houdini")
        reg.register("sphere", description="d", category="geo", dcc="houdini")
        assert "sim" in reg.get_categories()
        reg.unregister("cook", dcc_name="houdini")
        assert "sim" not in reg.get_categories()


class TestGetTags:
    def test_empty_returns_empty(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        assert reg.get_tags() == []

    def test_tags_sorted(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags()
        assert tags == sorted(tags)

    def test_tags_unique(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags()
        assert len(tags) == len(set(tags))

    def test_tags_all_dccs(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags()
        expected = {"create", "mesh", "delete", "render", "gpu", "hd", "bake", "anim", "sim", "hpc"}
        assert set(tags) == expected

    def test_tags_for_maya(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags(dcc_name="maya")
        assert set(tags) == {"create", "mesh", "delete", "render", "gpu", "hd"}

    def test_tags_for_blender(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags(dcc_name="blender")
        assert set(tags) == {"create", "mesh", "bake", "anim"}

    def test_tags_for_houdini(self) -> None:
        reg = _populate_registry()
        tags = reg.get_tags(dcc_name="houdini")
        assert set(tags) == {"sim", "hpc"}

    def test_tags_unknown_dcc_empty(self) -> None:
        reg = _populate_registry()
        assert reg.get_tags(dcc_name="unreal") == []

    def test_tags_no_tags_action_excluded(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("no_tag_action", description="d", category="misc", dcc="maya")
        assert reg.get_tags() == []

    def test_tags_after_batch_register(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register_batch(
            [
                {"name": "op1", "category": "c", "dcc": "maya", "tags": ["alpha", "beta"]},
                {"name": "op2", "category": "c", "dcc": "maya", "tags": ["beta", "gamma"]},
            ]
        )
        tags = reg.get_tags()
        assert set(tags) == {"alpha", "beta", "gamma"}

    def test_tags_sorted_after_multiple_register(self) -> None:
        reg = dcc_mcp_core.ToolRegistry()
        reg.register("z_op", description="d", category="c", dcc="maya", tags=["z_tag"])
        reg.register("a_op", description="d", category="c", dcc="maya", tags=["a_tag"])
        tags = reg.get_tags()
        assert tags == sorted(tags)
