"""Tests for MCP protocol types — full getter/setter coverage."""

# Import future modules
from __future__ import annotations

# Import local modules
import dcc_mcp_core


class TestToolDefinition:
    def test_create(self) -> None:
        td = dcc_mcp_core.ToolDefinition(
            name="create_sphere",
            description="Create a sphere",
            input_schema='{"type": "object"}',
        )
        assert td.name == "create_sphere"
        assert td.description == "Create a sphere"
        assert td.input_schema == '{"type": "object"}'
        assert td.output_schema is None

    def test_create_with_output_schema(self) -> None:
        td = dcc_mcp_core.ToolDefinition(
            name="t",
            description="d",
            input_schema="{}",
            output_schema='{"type": "object"}',
        )
        assert td.output_schema == '{"type": "object"}'

    def test_setters(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="old", description="old", input_schema="{}")
        td.name = "new_name"
        td.description = "new_desc"
        assert td.name == "new_name"
        assert td.description == "new_desc"

    def test_repr(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="test", description="d", input_schema="{}")
        assert "test" in repr(td)

    def test_equality(self) -> None:
        td1 = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        td2 = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        assert td1 == td2

    def test_inequality(self) -> None:
        td1 = dcc_mcp_core.ToolDefinition(name="a", description="d", input_schema="{}")
        td2 = dcc_mcp_core.ToolDefinition(name="b", description="d", input_schema="{}")
        assert td1 != td2

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(title="My Tool", read_only_hint=True)
        td = dcc_mcp_core.ToolDefinition(
            name="list_objects",
            description="List scene objects",
            input_schema="{}",
            annotations=ann,
        )
        assert td.annotations is not None
        assert td.annotations.title == "My Tool"
        assert td.annotations.read_only_hint is True

    def test_annotations_default_none(self) -> None:
        td = dcc_mcp_core.ToolDefinition(name="t", description="d", input_schema="{}")
        assert td.annotations is None


class TestToolAnnotations:
    def test_defaults(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations()
        assert ann.title is None
        assert ann.read_only_hint is None
        assert ann.destructive_hint is None
        assert ann.idempotent_hint is None
        assert ann.open_world_hint is None

    def test_all_values(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(
            title="Tool",
            read_only_hint=True,
            destructive_hint=False,
            idempotent_hint=True,
            open_world_hint=False,
        )
        assert ann.title == "Tool"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is False

    def test_setters(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations()
        ann.title = "New Title"
        ann.read_only_hint = True
        ann.destructive_hint = False
        ann.idempotent_hint = True
        ann.open_world_hint = False
        assert ann.title == "New Title"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False
        assert ann.idempotent_hint is True
        assert ann.open_world_hint is False

    def test_set_none(self) -> None:
        ann = dcc_mcp_core.ToolAnnotations(title="x")
        ann.title = None
        assert ann.title is None


class TestResourceAnnotations:
    def test_defaults(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations()
        assert ann.audience == []
        assert ann.priority is None

    def test_with_audience(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user", "assistant"])
        assert "user" in ann.audience
        assert "assistant" in ann.audience

    def test_with_priority(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(priority=0.8)
        assert ann.priority is not None
        assert abs(ann.priority - 0.8) < 1e-6

    def test_full(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"], priority=1.0)
        assert len(ann.audience) == 1
        assert ann.priority is not None
        assert abs(ann.priority - 1.0) < 1e-6

    def test_repr(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"])
        assert "ResourceAnnotations" in repr(ann)


class TestResourceDefinition:
    def test_create(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="file:///test.txt", name="test", description="A test")
        assert rd.uri == "file:///test.txt"
        assert rd.name == "test"
        assert rd.description == "A test"
        assert rd.mime_type == "text/plain"

    def test_custom_mime_type(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="u", name="n", description="d", mime_type="application/json")
        assert rd.mime_type == "application/json"

    def test_setters(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="old", name="old", description="old")
        rd.uri = "new_uri"
        rd.name = "new_name"
        rd.description = "new_desc"
        rd.mime_type = "image/png"
        assert rd.uri == "new_uri"
        assert rd.name == "new_name"
        assert rd.description == "new_desc"
        assert rd.mime_type == "image/png"

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(audience=["user"], priority=0.5)
        rd = dcc_mcp_core.ResourceDefinition(
            uri="scene://current",
            name="scene",
            description="Current scene data",
            mime_type="application/json",
            annotations=ann,
        )
        assert rd.annotations is not None
        assert "user" in rd.annotations.audience

    def test_annotations_default_none(self) -> None:
        rd = dcc_mcp_core.ResourceDefinition(uri="u", name="n", description="d")
        assert rd.annotations is None


class TestResourceTemplateDefinition:
    def test_create(self) -> None:
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="file:///{path}",
            name="template",
            description="A template",
        )
        assert rtd.uri_template == "file:///{path}"
        assert rtd.name == "template"
        assert rtd.description == "A template"
        assert rtd.mime_type == "text/plain"

    def test_setters(self) -> None:
        rtd = dcc_mcp_core.ResourceTemplateDefinition(uri_template="old", name="old", description="old")
        rtd.uri_template = "new/{id}"
        rtd.name = "new"
        rtd.description = "new desc"
        rtd.mime_type = "application/xml"
        assert rtd.uri_template == "new/{id}"
        assert rtd.name == "new"
        assert rtd.description == "new desc"
        assert rtd.mime_type == "application/xml"

    def test_with_annotations(self) -> None:
        ann = dcc_mcp_core.ResourceAnnotations(priority=0.9)
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="dcc://{dcc}/{object}",
            name="dcc_object",
            description="DCC scene object",
            annotations=ann,
        )
        assert rtd.annotations is not None
        assert rtd.annotations.priority is not None
        assert abs(rtd.annotations.priority - 0.9) < 1e-6


class TestPromptArgument:
    def test_create_default(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="An argument")
        assert pa.name == "arg1"
        assert pa.description == "An argument"
        assert pa.required is False

    def test_required(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="Req", required=True)
        assert pa.required is True

    def test_setters(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="old", description="old")
        pa.name = "new"
        pa.description = "new desc"
        pa.required = True
        assert pa.name == "new"
        assert pa.description == "new desc"
        assert pa.required is True

    def test_equality(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        pa2 = dcc_mcp_core.PromptArgument(name="x", description="d", required=True)
        assert pa1 == pa2

    def test_inequality(self) -> None:
        pa1 = dcc_mcp_core.PromptArgument(name="x", description="d")
        pa2 = dcc_mcp_core.PromptArgument(name="y", description="d")
        assert pa1 != pa2

    def test_repr(self) -> None:
        pa = dcc_mcp_core.PromptArgument(name="scene_name", description="The scene")
        assert "scene_name" in repr(pa)


class TestPromptDefinition:
    def test_create(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="my_prompt", description="A prompt")
        assert pd.name == "my_prompt"
        assert pd.description == "A prompt"

    def test_setters(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="old", description="old")
        pd.name = "new"
        pd.description = "new desc"
        assert pd.name == "new"
        assert pd.description == "new desc"

    def test_with_arguments(self) -> None:
        args = [
            dcc_mcp_core.PromptArgument(name="scene_path", description="Path to scene", required=True),
            dcc_mcp_core.PromptArgument(name="frame", description="Frame number"),
        ]
        pd = dcc_mcp_core.PromptDefinition(name="render_scene", description="Render a DCC scene", arguments=args)
        assert len(pd.arguments) == 2
        assert pd.arguments[0].name == "scene_path"
        assert pd.arguments[0].required is True
        assert pd.arguments[1].name == "frame"
        assert pd.arguments[1].required is False

    def test_arguments_default_empty(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="p", description="d")
        assert pd.arguments == []

    def test_equality(self) -> None:
        pd1 = dcc_mcp_core.PromptDefinition(name="p", description="d")
        pd2 = dcc_mcp_core.PromptDefinition(name="p", description="d")
        assert pd1 == pd2

    def test_inequality(self) -> None:
        pd1 = dcc_mcp_core.PromptDefinition(name="a", description="d")
        pd2 = dcc_mcp_core.PromptDefinition(name="b", description="d")
        assert pd1 != pd2

    def test_repr(self) -> None:
        pd = dcc_mcp_core.PromptDefinition(name="render_prompt", description="Render")
        assert "render_prompt" in repr(pd)
