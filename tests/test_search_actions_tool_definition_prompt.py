"""Tests for ToolRegistry.search_actions, protocol types, and McpHttpServer Python API.

Covers deep coverage of:
- ToolRegistry.search_actions: category/tags/dcc_name filters, AND logic, combined, empty
- search_actions: category/tags/dcc_name filters, AND logic, combined, empty
- result dict structure and field types
- ToolDefinition construction, input_schema, output_schema, annotations
- ToolAnnotations all fields, defaults
- ResourceDefinition construction, mime_type inference, annotations
- PromptDefinition construction, arguments
- McpHttpConfig server_version, repr edge cases
"""

from __future__ import annotations

import json

import pytest

from dcc_mcp_core import McpHttpConfig
from dcc_mcp_core import McpHttpServer
from dcc_mcp_core import PromptDefinition
from dcc_mcp_core import ResourceDefinition
from dcc_mcp_core import ToolAnnotations
from dcc_mcp_core import ToolDefinition
from dcc_mcp_core import ToolRegistry

# ── Fixtures ─────────────────────────────────────────────────────────────────

SCHEMA = '{"type": "object", "properties": {}}'


def _make_registry() -> ToolRegistry:
    """Five-action multi-DCC registry for search tests."""
    reg = ToolRegistry()
    reg.register_batch(
        [
            {
                "name": "create_sphere",
                "category": "geometry",
                "dcc": "maya",
                "tags": ["create", "mesh"],
                "description": "Create a sphere",
            },
            {
                "name": "delete_mesh",
                "category": "edit",
                "dcc": "maya",
                "tags": ["delete", "mesh"],
                "description": "Delete a mesh",
            },
            {
                "name": "create_cube",
                "category": "geometry",
                "dcc": "blender",
                "tags": ["create", "mesh"],
                "description": "Create a cube",
            },
            {
                "name": "render_frame",
                "category": "render",
                "dcc": "maya",
                "tags": ["render"],
                "description": "Render a frame",
            },
            {
                "name": "select_all",
                "category": "edit",
                "dcc": "blender",
                "tags": ["select"],
                "description": "Select all objects",
            },
        ]
    )
    return reg


# ── search_actions: basic filter ──────────────────────────────────────────────


class TestSearchActionsCategory:
    def test_by_category_returns_list(self):
        reg = _make_registry()
        result = reg.search_actions(category="geometry")
        assert isinstance(result, list)

    def test_by_category_geometry_count(self):
        reg = _make_registry()
        result = reg.search_actions(category="geometry")
        assert len(result) == 2

    def test_by_category_geometry_names(self):
        reg = _make_registry()
        names = {a["name"] for a in reg.search_actions(category="geometry")}
        assert names == {"create_sphere", "create_cube"}

    def test_by_category_edit_count(self):
        reg = _make_registry()
        assert len(reg.search_actions(category="edit")) == 2

    def test_by_category_render_count(self):
        reg = _make_registry()
        assert len(reg.search_actions(category="render")) == 1

    def test_by_category_render_name(self):
        reg = _make_registry()
        result = reg.search_actions(category="render")
        assert result[0]["name"] == "render_frame"

    def test_by_category_nonexistent_empty(self):
        reg = _make_registry()
        assert reg.search_actions(category="nonexistent") == []

    def test_by_category_results_are_dicts(self):
        reg = _make_registry()
        for item in reg.search_actions(category="geometry"):
            assert isinstance(item, dict)

    def test_by_category_each_result_has_name(self):
        reg = _make_registry()
        for item in reg.search_actions(category="geometry"):
            assert "name" in item

    def test_by_category_each_result_category_matches(self):
        reg = _make_registry()
        for item in reg.search_actions(category="geometry"):
            assert item["category"] == "geometry"


class TestSearchActionsTag:
    def test_by_single_tag_mesh_count(self):
        reg = _make_registry()
        result = reg.search_actions(tags=["mesh"])
        assert len(result) == 3  # create_sphere, delete_mesh, create_cube

    def test_by_single_tag_create_count(self):
        reg = _make_registry()
        assert len(reg.search_actions(tags=["create"])) == 2

    def test_by_single_tag_render(self):
        reg = _make_registry()
        result = reg.search_actions(tags=["render"])
        assert len(result) == 1
        assert result[0]["name"] == "render_frame"

    def test_by_single_tag_nonexistent_empty(self):
        reg = _make_registry()
        assert reg.search_actions(tags=["nonexistent_tag"]) == []

    def test_tags_and_logic_two_tags(self):
        """Tags filter requires ALL tags to be present (AND logic)."""
        reg = ToolRegistry()
        reg.register("all_tags", description="d", category="c", dcc="maya", tags=["a", "b", "c"])
        reg.register("two_tags", description="d", category="c", dcc="maya", tags=["a", "b"])
        reg.register("one_tag", description="d", category="c", dcc="maya", tags=["a"])
        result = reg.search_actions(tags=["a", "b"])
        names = {a["name"] for a in result}
        assert names == {"all_tags", "two_tags"}

    def test_tags_and_logic_three_tags(self):
        reg = ToolRegistry()
        reg.register("all_three", description="d", category="c", dcc="maya", tags=["a", "b", "c"])
        reg.register("only_ab", description="d", category="c", dcc="maya", tags=["a", "b"])
        result = reg.search_actions(tags=["a", "b", "c"])
        assert len(result) == 1
        assert result[0]["name"] == "all_three"

    def test_empty_tags_returns_all(self):
        reg = _make_registry()
        assert len(reg.search_actions(tags=[])) == 5

    def test_tags_result_field_is_list(self):
        reg = _make_registry()
        for item in reg.search_actions(tags=["mesh"]):
            assert isinstance(item["tags"], list)

    def test_tags_result_contains_queried_tag(self):
        reg = _make_registry()
        for item in reg.search_actions(tags=["mesh"]):
            assert "mesh" in item["tags"]


class TestSearchActionsDcc:
    def test_by_dcc_maya_count(self):
        reg = _make_registry()
        assert len(reg.search_actions(dcc_name="maya")) == 3

    def test_by_dcc_blender_count(self):
        reg = _make_registry()
        assert len(reg.search_actions(dcc_name="blender")) == 2

    def test_by_dcc_nonexistent_empty(self):
        reg = _make_registry()
        assert reg.search_actions(dcc_name="3dsmax") == []

    def test_by_dcc_result_dcc_field_matches(self):
        reg = _make_registry()
        for item in reg.search_actions(dcc_name="maya"):
            assert item["dcc"] == "maya"

    def test_by_dcc_isolates_shared_action_name(self):
        reg = ToolRegistry()
        reg.register("shared_op", description="d", category="c", dcc="maya")
        reg.register("shared_op", description="d", category="c", dcc="blender")
        maya_results = reg.search_actions(dcc_name="maya")
        blender_results = reg.search_actions(dcc_name="blender")
        assert len(maya_results) == 1
        assert len(blender_results) == 1
        assert maya_results[0]["dcc"] == "maya"
        assert blender_results[0]["dcc"] == "blender"


class TestSearchActionsCombined:
    def test_category_and_dcc_maya_geometry(self):
        reg = _make_registry()
        result = reg.search_actions(category="geometry", dcc_name="maya")
        assert len(result) == 1
        assert result[0]["name"] == "create_sphere"

    def test_category_and_dcc_blender_geometry(self):
        reg = _make_registry()
        result = reg.search_actions(category="geometry", dcc_name="blender")
        assert len(result) == 1
        assert result[0]["name"] == "create_cube"

    def test_category_and_dcc_no_match(self):
        reg = _make_registry()
        assert reg.search_actions(category="render", dcc_name="blender") == []

    def test_triple_filter_category_dcc_tag(self):
        reg = _make_registry()
        result = reg.search_actions(category="geometry", dcc_name="maya", tags=["create"])
        assert len(result) == 1
        assert result[0]["name"] == "create_sphere"

    def test_triple_filter_no_match(self):
        reg = _make_registry()
        assert reg.search_actions(category="geometry", dcc_name="maya", tags=["delete"]) == []

    def test_no_filters_returns_all(self):
        reg = _make_registry()
        assert len(reg.search_actions()) == 5

    def test_after_unregister_search_updated(self):
        reg = _make_registry()
        reg.unregister("create_sphere", dcc_name="maya")
        result = reg.search_actions(category="geometry", dcc_name="maya")
        assert result == []

    def test_empty_registry_all_searches_empty(self):
        reg = ToolRegistry()
        assert reg.search_actions() == []
        assert reg.search_actions(category="x") == []
        assert reg.search_actions(dcc_name="maya") == []
        assert reg.search_actions(tags=["x"]) == []


class TestSearchActionsResultStructure:
    def test_result_has_all_required_keys(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        required_keys = {
            "name",
            "category",
            "dcc",
            "description",
            "tags",
            "version",
            "input_schema",
            "output_schema",
            "source_file",
        }
        assert required_keys.issubset(set(item.keys()))

    def test_result_name_is_str(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["name"], str)

    def test_result_category_is_str(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["category"], str)

    def test_result_dcc_is_str(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["dcc"], str)

    def test_result_version_is_str(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["version"], str)

    def test_result_version_default(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert item["version"] == "1.0.0"

    def test_result_version_custom(self):
        reg = ToolRegistry()
        reg.register("v_action", description="d", category="c", dcc="maya", version="3.1.0")
        item = reg.search_actions(dcc_name="maya")[0]
        assert item["version"] == "3.1.0"

    def test_result_tags_is_list(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["tags"], list)

    def test_result_input_schema_is_dict(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert isinstance(item["input_schema"], dict)

    def test_result_source_file_is_none_for_programmatic(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry")[0]
        assert item["source_file"] is None

    def test_result_description_is_str(self):
        reg = _make_registry()
        item = reg.search_actions(category="geometry", dcc_name="maya")[0]
        assert isinstance(item["description"], str)
        assert len(item["description"]) > 0


# ── ToolDefinition ────────────────────────────────────────────────────────────


class TestToolDefinitionBasic:
    def test_construction_name_description_schema(self):
        td = ToolDefinition(name="my_tool", description="A tool", input_schema=SCHEMA)
        assert td.name == "my_tool"

    def test_name_is_str(self):
        td = ToolDefinition(name="tool_name", description="d", input_schema=SCHEMA)
        assert isinstance(td.name, str)

    def test_description_is_str(self):
        td = ToolDefinition(name="t", description="my description", input_schema=SCHEMA)
        assert td.description == "my description"

    def test_input_schema_stored(self):
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA)
        assert td.input_schema == SCHEMA

    def test_output_schema_default_none(self):
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA)
        assert td.output_schema is None

    def test_output_schema_set(self):
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA, output_schema=SCHEMA)
        assert td.output_schema == SCHEMA

    def test_annotations_default_none(self):
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA)
        assert td.annotations is None

    def test_repr_contains_name(self):
        td = ToolDefinition(name="fancy_tool", description="d", input_schema=SCHEMA)
        assert "fancy_tool" in repr(td)

    def test_repr_contains_tool_definition(self):
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA)
        assert "ToolDefinition" in repr(td)

    def test_different_names(self):
        td1 = ToolDefinition(name="tool_a", description="d", input_schema=SCHEMA)
        td2 = ToolDefinition(name="tool_b", description="d", input_schema=SCHEMA)
        assert td1.name != td2.name


class TestToolDefinitionWithAnnotations:
    def test_annotations_attached(self):
        ta = ToolAnnotations(title="My Tool")
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA, annotations=ta)
        assert td.annotations is not None

    def test_annotations_title_accessible(self):
        ta = ToolAnnotations(title="Annotated Tool")
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA, annotations=ta)
        assert td.annotations.title == "Annotated Tool"

    def test_annotations_read_only_hint(self):
        ta = ToolAnnotations(read_only_hint=True)
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA, annotations=ta)
        assert td.annotations.read_only_hint is True

    def test_annotations_destructive_hint(self):
        ta = ToolAnnotations(destructive_hint=True)
        td = ToolDefinition(name="t", description="d", input_schema=SCHEMA, annotations=ta)
        assert td.annotations.destructive_hint is True


# ── ToolAnnotations ───────────────────────────────────────────────────────────


class TestToolAnnotations:
    def test_all_defaults_none(self):
        ta = ToolAnnotations()
        assert ta.title is None
        assert ta.read_only_hint is None
        assert ta.destructive_hint is None
        assert ta.idempotent_hint is None
        assert ta.open_world_hint is None

    def test_title_set(self):
        ta = ToolAnnotations(title="My Tool")
        assert ta.title == "My Tool"

    def test_title_is_str(self):
        ta = ToolAnnotations(title="Title")
        assert isinstance(ta.title, str)

    def test_read_only_hint_true(self):
        ta = ToolAnnotations(read_only_hint=True)
        assert ta.read_only_hint is True

    def test_read_only_hint_false(self):
        ta = ToolAnnotations(read_only_hint=False)
        assert ta.read_only_hint is False

    def test_destructive_hint_true(self):
        ta = ToolAnnotations(destructive_hint=True)
        assert ta.destructive_hint is True

    def test_destructive_hint_false(self):
        ta = ToolAnnotations(destructive_hint=False)
        assert ta.destructive_hint is False

    def test_idempotent_hint_true(self):
        ta = ToolAnnotations(idempotent_hint=True)
        assert ta.idempotent_hint is True

    def test_idempotent_hint_false(self):
        ta = ToolAnnotations(idempotent_hint=False)
        assert ta.idempotent_hint is False

    def test_open_world_hint_true(self):
        ta = ToolAnnotations(open_world_hint=True)
        assert ta.open_world_hint is True

    def test_open_world_hint_false(self):
        ta = ToolAnnotations(open_world_hint=False)
        assert ta.open_world_hint is False

    def test_all_fields_set(self):
        ta = ToolAnnotations(
            title="T",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        assert ta.title == "T"
        assert ta.read_only_hint is True
        assert ta.destructive_hint is False
        assert ta.idempotent_hint is True
        assert ta.open_world_hint is False

    def test_two_instances_independent(self):
        ta1 = ToolAnnotations(title="A", read_only_hint=True)
        ta2 = ToolAnnotations(title="B", read_only_hint=False)
        assert ta1.title != ta2.title
        assert ta1.read_only_hint is not ta2.read_only_hint


# ── ResourceDefinition ───────────────────────────────────────────────────────


class TestResourceDefinition:
    def test_name_is_set(self):
        rd = ResourceDefinition(name="res", uri="file:///path.txt", description="d")
        assert rd.name == "res"

    def test_uri_is_set(self):
        rd = ResourceDefinition(name="res", uri="file:///data.json", description="d")
        assert rd.uri == "file:///data.json"

    def test_description_is_set(self):
        rd = ResourceDefinition(name="res", uri="file:///f.txt", description="my resource")
        assert rd.description == "my resource"

    def test_mime_type_default_for_txt(self):
        rd = ResourceDefinition(name="res", uri="file:///f.txt", description="d")
        assert rd.mime_type == "text/plain"

    def test_mime_type_custom(self):
        rd = ResourceDefinition(
            name="res",
            uri="file:///data.json",
            description="d",
            mime_type="application/json",
        )
        assert rd.mime_type == "application/json"

    def test_mime_type_xml(self):
        rd = ResourceDefinition(
            name="res",
            uri="file:///doc.xml",
            description="d",
            mime_type="application/xml",
        )
        assert rd.mime_type == "application/xml"

    def test_annotations_default_none(self):
        rd = ResourceDefinition(name="res", uri="file:///f.txt", description="d")
        assert rd.annotations is None

    def test_repr_contains_name(self):
        rd = ResourceDefinition(name="my_resource", uri="file:///f.txt", description="d")
        assert "my_resource" in repr(rd)

    def test_repr_contains_uri(self):
        rd = ResourceDefinition(name="res", uri="file:///path/to/file.txt", description="d")
        assert "file:///path/to/file.txt" in repr(rd)

    def test_repr_contains_resource_definition(self):
        rd = ResourceDefinition(name="r", uri="file:///f.txt", description="d")
        assert "ResourceDefinition" in repr(rd)

    def test_name_is_str(self):
        rd = ResourceDefinition(name="r", uri="file:///f.txt", description="d")
        assert isinstance(rd.name, str)

    def test_uri_is_str(self):
        rd = ResourceDefinition(name="r", uri="file:///f.txt", description="d")
        assert isinstance(rd.uri, str)

    def test_two_different_resources(self):
        rd1 = ResourceDefinition(name="r1", uri="file:///a.txt", description="d1")
        rd2 = ResourceDefinition(name="r2", uri="file:///b.txt", description="d2")
        assert rd1.name != rd2.name
        assert rd1.uri != rd2.uri


# ── PromptDefinition ─────────────────────────────────────────────────────────


class TestPromptDefinition:
    def test_name_is_set(self):
        pd = PromptDefinition(name="my_prompt", description="d")
        assert pd.name == "my_prompt"

    def test_description_is_set(self):
        pd = PromptDefinition(name="p", description="my description")
        assert pd.description == "my description"

    def test_name_is_str(self):
        pd = PromptDefinition(name="prompt_name", description="d")
        assert isinstance(pd.name, str)

    def test_description_is_str(self):
        pd = PromptDefinition(name="p", description="desc")
        assert isinstance(pd.description, str)

    def test_arguments_default_empty_list(self):
        pd = PromptDefinition(name="p", description="d")
        assert pd.arguments == []

    def test_repr_contains_name(self):
        pd = PromptDefinition(name="named_prompt", description="d")
        assert "named_prompt" in repr(pd)

    def test_repr_contains_prompt_definition(self):
        pd = PromptDefinition(name="p", description="d")
        assert "PromptDefinition" in repr(pd)

    def test_two_different_prompts(self):
        pd1 = PromptDefinition(name="p1", description="d1")
        pd2 = PromptDefinition(name="p2", description="d2")
        assert pd1.name != pd2.name
        assert pd1.description != pd2.description


# ── McpHttpConfig extra tests ─────────────────────────────────────────────────


class TestMcpHttpConfigExtra:
    def test_default_port_is_8765(self):
        cfg = McpHttpConfig()
        assert cfg.port == 8765

    def test_default_server_name(self):
        cfg = McpHttpConfig()
        assert cfg.server_name == "dcc-mcp"

    def test_server_version_default_nonempty(self):
        cfg = McpHttpConfig()
        assert isinstance(cfg.server_version, str)
        assert len(cfg.server_version) > 0

    def test_server_version_custom(self):
        cfg = McpHttpConfig(port=8765, server_version="2.0.0")
        assert cfg.server_version == "2.0.0"

    def test_port_custom(self):
        cfg = McpHttpConfig(port=9999)
        assert cfg.port == 9999

    def test_server_name_custom(self):
        cfg = McpHttpConfig(server_name="my-server")
        assert cfg.server_name == "my-server"

    def test_repr_contains_port(self):
        cfg = McpHttpConfig(port=1234)
        assert "1234" in repr(cfg)

    def test_repr_contains_server_name(self):
        cfg = McpHttpConfig(server_name="test-srv")
        assert "test-srv" in repr(cfg)

    def test_repr_contains_mcp_http_config(self):
        cfg = McpHttpConfig()
        assert "McpHttpConfig" in repr(cfg)

    def test_port_zero_for_random(self):
        cfg = McpHttpConfig(port=0)
        assert cfg.port == 0

    def test_two_configs_independent(self):
        cfg1 = McpHttpConfig(port=8001, server_name="srv1")
        cfg2 = McpHttpConfig(port=8002, server_name="srv2")
        assert cfg1.port != cfg2.port
        assert cfg1.server_name != cfg2.server_name


class TestMcpHttpServerExtra:
    def test_server_repr_contains_server_name(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=8765, server_name="extra-test")
        server = McpHttpServer(reg, cfg)
        assert "extra-test" in repr(server)

    def test_server_repr_contains_mcp_http_server(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=8765)
        server = McpHttpServer(reg, cfg)
        assert "McpHttpServer" in repr(server)

    def test_empty_registry_server_created(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        assert server is not None

    def test_server_start_returns_handle_with_port(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        assert handle.port > 0
        handle.shutdown()

    def test_server_handle_mcp_url_format(self):
        reg = ToolRegistry()
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        url = handle.mcp_url()
        assert url.startswith("http://")
        assert "/mcp" in url
        handle.shutdown()

    def test_server_with_multiple_actions(self):
        reg = ToolRegistry()
        reg.register_batch(
            [
                {"name": "action_a", "description": "a", "category": "c", "dcc": "test"},
                {"name": "action_b", "description": "b", "category": "c", "dcc": "test"},
                {"name": "action_c", "description": "c", "category": "c", "dcc": "test"},
            ]
        )
        cfg = McpHttpConfig(port=0)
        server = McpHttpServer(reg, cfg)
        handle = server.start()
        assert handle.port > 0
        handle.shutdown()
