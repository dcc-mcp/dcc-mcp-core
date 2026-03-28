"""Tests for MCP protocol types — full getter/setter coverage."""

# Import local modules
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

    def test_create_with_output_schema(self):
        td = dcc_mcp_core.ToolDefinition(
            name="t",
            description="d",
            input_schema="{}",
            output_schema='{"type": "object"}',
        )
        assert td.output_schema == '{"type": "object"}'

    def test_setters(self):
        td = dcc_mcp_core.ToolDefinition(name="old", description="old", input_schema="{}")
        td.name = "new_name"
        td.description = "new_desc"
        assert td.name == "new_name"
        assert td.description == "new_desc"

    def test_repr(self):
        td = dcc_mcp_core.ToolDefinition(name="test", description="d", input_schema="{}")
        assert "test" in repr(td)


class TestToolAnnotations:
    def test_defaults(self):
        ann = dcc_mcp_core.ToolAnnotations()
        assert ann.title is None
        assert ann.read_only_hint is None
        assert ann.destructive_hint is None
        assert ann.idempotent_hint is None
        assert ann.open_world_hint is None

    def test_all_values(self):
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

    def test_setters(self):
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

    def test_set_none(self):
        ann = dcc_mcp_core.ToolAnnotations(title="x")
        ann.title = None
        assert ann.title is None


class TestResourceDefinition:
    def test_create(self):
        rd = dcc_mcp_core.ResourceDefinition(
            uri="file:///test.txt", name="test", description="A test"
        )
        assert rd.uri == "file:///test.txt"
        assert rd.name == "test"
        assert rd.description == "A test"
        assert rd.mime_type == "text/plain"

    def test_custom_mime_type(self):
        rd = dcc_mcp_core.ResourceDefinition(
            uri="u", name="n", description="d", mime_type="application/json"
        )
        assert rd.mime_type == "application/json"

    def test_setters(self):
        rd = dcc_mcp_core.ResourceDefinition(uri="old", name="old", description="old")
        rd.uri = "new_uri"
        rd.name = "new_name"
        rd.description = "new_desc"
        rd.mime_type = "image/png"
        assert rd.uri == "new_uri"
        assert rd.name == "new_name"
        assert rd.description == "new_desc"
        assert rd.mime_type == "image/png"


class TestResourceTemplateDefinition:
    def test_create(self):
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="file:///{path}",
            name="template",
            description="A template",
        )
        assert rtd.uri_template == "file:///{path}"
        assert rtd.name == "template"
        assert rtd.description == "A template"
        assert rtd.mime_type == "text/plain"

    def test_setters(self):
        rtd = dcc_mcp_core.ResourceTemplateDefinition(
            uri_template="old", name="old", description="old"
        )
        rtd.uri_template = "new/{id}"
        rtd.name = "new"
        rtd.description = "new desc"
        rtd.mime_type = "application/xml"
        assert rtd.uri_template == "new/{id}"
        assert rtd.name == "new"
        assert rtd.description == "new desc"
        assert rtd.mime_type == "application/xml"


class TestPromptArgument:
    def test_create_default(self):
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="An argument")
        assert pa.name == "arg1"
        assert pa.description == "An argument"
        assert pa.required is False

    def test_required(self):
        pa = dcc_mcp_core.PromptArgument(name="arg1", description="Req", required=True)
        assert pa.required is True

    def test_setters(self):
        pa = dcc_mcp_core.PromptArgument(name="old", description="old")
        pa.name = "new"
        pa.description = "new desc"
        pa.required = True
        assert pa.name == "new"
        assert pa.description == "new desc"
        assert pa.required is True


class TestPromptDefinition:
    def test_create(self):
        pd = dcc_mcp_core.PromptDefinition(name="my_prompt", description="A prompt")
        assert pd.name == "my_prompt"
        assert pd.description == "A prompt"

    def test_setters(self):
        pd = dcc_mcp_core.PromptDefinition(name="old", description="old")
        pd.name = "new"
        pd.description = "new desc"
        assert pd.name == "new"
        assert pd.description == "new desc"
