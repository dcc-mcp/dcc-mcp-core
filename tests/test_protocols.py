"""Tests for MCP protocol types, base classes, and adapters.

This module tests the MCP protocol integration in dcc-mcp-core.
"""

# Import built-in modules
from typing import Any
from typing import ClassVar
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import Field
import pytest

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.protocols import MCPAdapter
from dcc_mcp_core.protocols import Prompt
from dcc_mcp_core.protocols import Resource
from dcc_mcp_core.protocols.types import PromptArgument
from dcc_mcp_core.protocols.types import PromptDefinition
from dcc_mcp_core.protocols.types import ResourceDefinition
from dcc_mcp_core.protocols.types import ToolAnnotations
from dcc_mcp_core.protocols.types import ToolDefinition


# Test Action class for conversion
class CreateSphereAction(Action):
    """Test action for creating a sphere."""

    name: ClassVar[str] = "create_sphere"
    description: ClassVar[str] = "Creates a polygon sphere in the scene"
    tags: ClassVar[List[str]] = ["geometry", "creation"]
    dcc: ClassVar[str] = "maya"
    read_only: ClassVar[bool] = False
    destructive: ClassVar[bool] = False

    class InputModel(Action.InputModel):
        """Input parameters for create_sphere."""

        radius: float = Field(1.0, description="Radius of the sphere")
        name: Optional[str] = Field(None, description="Name of the sphere")

    class OutputModel(Action.OutputModel):
        """Output data for create_sphere."""

        object_name: str = Field(description="Name of the created object")
        position: List[float] = Field(description="Position of the object")

    def _execute(self) -> None:
        self.output = self.OutputModel(
            object_name=self.input.name or "sphere1",
            position=[0, 0, 0],
            prompt="Sphere created successfully",
        )


class ReadOnlyAction(Action):
    """Test action that is read-only."""

    name: ClassVar[str] = "get_scene_info"
    description: ClassVar[str] = "Gets information about the current scene"
    dcc: ClassVar[str] = "maya"
    read_only: ClassVar[bool] = True

    class InputModel(Action.InputModel):
        """Input parameters."""

    class OutputModel(Action.OutputModel):
        """Output data."""

        object_count: int = Field(description="Number of objects")

    def _execute(self) -> None:
        self.output = self.OutputModel(object_count=10)


# Test Resource class
class SceneObjectsResource(Resource):
    """Test resource for scene objects."""

    uri: ClassVar[str] = "scene://objects"
    name: ClassVar[str] = "Scene Objects"
    description: ClassVar[str] = "List of all objects in the scene"
    mime_type: ClassVar[str] = "application/json"
    dcc: ClassVar[str] = "maya"

    def read(self, **params: Any) -> str:
        return '["cube1", "sphere1", "camera1"]'


class DynamicResource(Resource):
    """Test resource with URI template."""

    uri_template: ClassVar[str] = "scene://objects/{category}/{name}"
    name: ClassVar[str] = "Object Details"
    description: ClassVar[str] = "Get details for a specific object"
    mime_type: ClassVar[str] = "application/json"
    dcc: ClassVar[str] = "maya"

    def read(self, **params: Any) -> str:
        category = params.get("category", "unknown")
        name = params.get("name", "unknown")
        return f'{{"category": "{category}", "name": "{name}"}}'


# Test Prompt class
class ModelReviewPrompt(Prompt):
    """Test prompt for model review."""

    name: ClassVar[str] = "model_review"
    description: ClassVar[str] = "Generate a prompt for reviewing a 3D model"
    dcc: ClassVar[str] = "maya"

    class ArgumentsModel(Prompt.ArgumentsModel):
        """Arguments for model review prompt."""

        object_name: str = Field(description="Name of the object to review")
        check_topology: bool = Field(True, description="Whether to check topology")

    def render(self, **kwargs: Any) -> str:
        args = self.ArgumentsModel(**kwargs)
        prompt = f"Please review the 3D model '{args.object_name}'."
        if args.check_topology:
            prompt += " Check the topology for issues."
        return prompt


class TestToolDefinition:
    """Tests for ToolDefinition type."""

    def test_create_tool_definition(self):
        """Test creating a ToolDefinition."""
        tool = ToolDefinition(
            name="test_tool",
            description="A test tool",
            inputSchema={"type": "object", "properties": {}},
        )
        assert tool.name == "test_tool"
        assert tool.description == "A test tool"
        assert tool.outputSchema is None
        assert tool.annotations is None

    def test_tool_definition_with_output_schema(self):
        """Test ToolDefinition with output schema."""
        tool = ToolDefinition(
            name="test_tool",
            description="A test tool",
            inputSchema={"type": "object"},
            outputSchema={"type": "object", "properties": {"result": {"type": "string"}}},
        )
        assert tool.outputSchema is not None
        assert "result" in tool.outputSchema["properties"]

    def test_tool_definition_with_annotations(self):
        """Test ToolDefinition with annotations."""
        annotations = ToolAnnotations(
            title="Test Tool",
            readOnlyHint=True,
            destructiveHint=False,
        )
        tool = ToolDefinition(
            name="test_tool",
            description="A test tool",
            inputSchema={"type": "object"},
            annotations=annotations,
        )
        assert tool.annotations.title == "Test Tool"
        assert tool.annotations.readOnlyHint is True
        assert tool.annotations.destructiveHint is False


class TestResourceDefinition:
    """Tests for ResourceDefinition type."""

    def test_create_resource_definition(self):
        """Test creating a ResourceDefinition."""
        resource = ResourceDefinition(
            uri="scene://objects",
            name="Scene Objects",
            description="List of objects",
            mimeType="application/json",
        )
        assert resource.uri == "scene://objects"
        assert resource.name == "Scene Objects"
        assert resource.mimeType == "application/json"

    def test_resource_definition_default_mime_type(self):
        """Test ResourceDefinition default MIME type."""
        resource = ResourceDefinition(
            uri="test://resource",
            name="Test",
            description="Test resource",
        )
        assert resource.mimeType == "text/plain"


class TestPromptDefinition:
    """Tests for PromptDefinition type."""

    def test_create_prompt_definition(self):
        """Test creating a PromptDefinition."""
        prompt = PromptDefinition(
            name="test_prompt",
            description="A test prompt",
        )
        assert prompt.name == "test_prompt"
        assert prompt.arguments is None

    def test_prompt_definition_with_arguments(self):
        """Test PromptDefinition with arguments."""
        prompt = PromptDefinition(
            name="test_prompt",
            description="A test prompt",
            arguments=[
                PromptArgument(name="arg1", description="First argument", required=True),
                PromptArgument(name="arg2", description="Second argument", required=False),
            ],
        )
        assert len(prompt.arguments) == 2
        assert prompt.arguments[0].name == "arg1"
        assert prompt.arguments[0].required is True


class TestResourceBase:
    """Tests for Resource base class."""

    def test_resource_read(self):
        """Test Resource read method."""
        resource = SceneObjectsResource()
        content = resource.read()
        assert "cube1" in content
        assert "sphere1" in content

    def test_resource_get_uri(self):
        """Test Resource get_uri method."""
        assert SceneObjectsResource.get_uri() == "scene://objects"

    def test_resource_is_template(self):
        """Test Resource is_template method."""
        assert SceneObjectsResource.is_template() is False
        assert DynamicResource.is_template() is True

    def test_dynamic_resource_read(self):
        """Test dynamic Resource read with parameters."""
        resource = DynamicResource()
        content = resource.read(category="meshes", name="cube1")
        assert "meshes" in content
        assert "cube1" in content


class TestPromptBase:
    """Tests for Prompt base class."""

    def test_prompt_render(self):
        """Test Prompt render method."""
        prompt = ModelReviewPrompt()
        result = prompt.render(object_name="cube1", check_topology=True)
        assert "cube1" in result
        assert "topology" in result

    def test_prompt_render_without_topology(self):
        """Test Prompt render without topology check."""
        prompt = ModelReviewPrompt()
        result = prompt.render(object_name="sphere1", check_topology=False)
        assert "sphere1" in result
        assert "topology" not in result

    def test_prompt_get_arguments_schema(self):
        """Test Prompt get_arguments_schema method."""
        schema = ModelReviewPrompt.get_arguments_schema()
        assert "properties" in schema
        assert "object_name" in schema["properties"]

    def test_prompt_get_required_arguments(self):
        """Test Prompt get_required_arguments method."""
        required = ModelReviewPrompt.get_required_arguments()
        assert "object_name" in required

    def test_prompt_render_messages(self):
        """Test Prompt render_messages method."""
        prompt = ModelReviewPrompt()
        messages = prompt.render_messages(object_name="cube1")
        assert len(messages) == 1
        assert messages[0]["role"] == "user"
        assert "cube1" in messages[0]["content"]


class TestMCPAdapter:
    """Tests for MCPAdapter."""

    def test_action_to_tool(self):
        """Test converting Action to Tool definition."""
        tool = MCPAdapter.action_to_tool(CreateSphereAction)

        assert tool.name == "create_sphere"
        assert tool.description == "Creates a polygon sphere in the scene"
        assert tool.inputSchema is not None
        assert "properties" in tool.inputSchema
        assert "radius" in tool.inputSchema["properties"]

    def test_action_to_tool_with_output_schema(self):
        """Test Action to Tool with output schema."""
        tool = MCPAdapter.action_to_tool(CreateSphereAction, include_output_schema=True)

        assert tool.outputSchema is not None
        assert "properties" in tool.outputSchema
        assert "object_name" in tool.outputSchema["properties"]
        # prompt should be excluded from output schema
        assert "prompt" not in tool.outputSchema["properties"]

    def test_action_to_tool_without_output_schema(self):
        """Test Action to Tool without output schema."""
        tool = MCPAdapter.action_to_tool(CreateSphereAction, include_output_schema=False)
        assert tool.outputSchema is None

    def test_action_to_tool_read_only(self):
        """Test Action to Tool with read-only hint."""
        tool = MCPAdapter.action_to_tool(ReadOnlyAction, include_annotations=True)

        assert tool.annotations is not None
        assert tool.annotations.readOnlyHint is True

    def test_resource_to_definition(self):
        """Test converting Resource to ResourceDefinition."""
        resource_def = MCPAdapter.resource_to_definition(SceneObjectsResource)

        assert resource_def.uri == "scene://objects"
        assert resource_def.name == "Scene Objects"
        assert resource_def.mimeType == "application/json"

    def test_resource_to_template_definition(self):
        """Test converting Resource with template to ResourceTemplateDefinition."""
        template_def = MCPAdapter.resource_to_template_definition(DynamicResource)

        assert template_def.uriTemplate == "scene://objects/{category}/{name}"
        assert template_def.name == "Object Details"

    def test_resource_to_template_definition_error(self):
        """Test error when converting non-template Resource."""
        with pytest.raises(ValueError, match="does not have a URI template"):
            MCPAdapter.resource_to_template_definition(SceneObjectsResource)

    def test_prompt_to_definition(self):
        """Test converting Prompt to PromptDefinition."""
        prompt_def = MCPAdapter.prompt_to_definition(ModelReviewPrompt)

        assert prompt_def.name == "model_review"
        assert prompt_def.description == "Generate a prompt for reviewing a 3D model"
        assert prompt_def.arguments is not None
        assert len(prompt_def.arguments) == 2

        # Check arguments
        arg_names = [arg.name for arg in prompt_def.arguments]
        assert "object_name" in arg_names
        assert "check_topology" in arg_names

    def test_parse_uri_template_params(self):
        """Test parsing URI template parameters."""
        params = MCPAdapter.parse_uri_template_params("scene://objects/{category}/{name}")
        assert params == ["category", "name"]

    def test_parse_uri_template_params_empty(self):
        """Test parsing URI template with no parameters."""
        params = MCPAdapter.parse_uri_template_params("scene://objects")
        assert params == []

    def test_match_uri_to_template(self):
        """Test matching URI to template."""
        result = MCPAdapter.match_uri_to_template(
            "scene://objects/meshes/cube1",
            "scene://objects/{category}/{name}",
        )
        assert result == {"category": "meshes", "name": "cube1"}

    def test_match_uri_to_template_no_match(self):
        """Test matching URI to template with no match."""
        result = MCPAdapter.match_uri_to_template(
            "other://path",
            "scene://objects/{category}/{name}",
        )
        assert result is None
