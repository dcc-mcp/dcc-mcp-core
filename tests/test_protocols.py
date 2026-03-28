"""Tests for MCP protocol types."""

import dcc_mcp_core


class TestToolDefinition:
    def test_create(self):
        td = dcc_mcp_core.ToolDefinition(
            name="create_sphere",
            description="Create a sphere",
            input_schema='{"type": "object"}',
        )
        assert td.name == "create_sphere"
        assert td.description == "Create a sphere"
        assert td.input_schema == '{"type": "object"}'
        assert td.output_schema is None

    def test_repr(self):
        td = dcc_mcp_core.ToolDefinition(
            name="test", description="desc", input_schema="{}"
        )
        assert "test" in repr(td)


class TestToolAnnotations:
    def test_defaults(self):
        ann = dcc_mcp_core.ToolAnnotations()
        assert ann.title is None
        assert ann.read_only_hint is None

    def test_set_values(self):
        ann = dcc_mcp_core.ToolAnnotations(
            title="My Tool", read_only_hint=True, destructive_hint=False
        )
        assert ann.title == "My Tool"
        assert ann.read_only_hint is True
        assert ann.destructive_hint is False


class TestResourceDefinition:
    def test_create(self):
        rd = dcc_mcp_core.ResourceDefinition(
            uri="file:///test.txt", name="test", description="A test resource"
        )
        assert rd.uri == "file:///test.txt"
        assert rd.mime_type == "text/plain"  # default


class TestResourceTemplateDefinition:
    def test_create(self):
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="file:///{path}",
            name="template",
            description="A template",
        )
        assert rtd.uri_template == "file:///{path}"


class TestPromptArgument:
    def test_create(self):
        pa = dcc_mcp_core.PromptArgument(
            name="arg1", description="An argument"
        )
        assert pa.name == "arg1"
        assert pa.required is False

    def test_required(self):
        pa = dcc_mcp_core.PromptArgument(
            name="arg1", description="Required arg", required=True
        )
        assert pa.required is True


class TestPromptDefinition:
    def test_create(self):
        pd = dcc_mcp_core.PromptDefinition(
            name="my_prompt", description="A prompt"
        )
        assert pd.name == "my_prompt"
