"""MCP Adapter for converting dcc-mcp-core primitives to MCP protocol types.

This module provides adapters to convert Action, Resource, and Prompt classes
to their corresponding MCP protocol definitions (Tool, Resource, Prompt).
"""

# Import built-in modules
import re
from typing import Any
from typing import Dict
from typing import List
from typing import Optional
from typing import Type

# Import local modules
from dcc_mcp_core.actions.base import Action
from dcc_mcp_core.protocols.base import Prompt
from dcc_mcp_core.protocols.base import Resource
from dcc_mcp_core.protocols.types import PromptArgument
from dcc_mcp_core.protocols.types import PromptDefinition
from dcc_mcp_core.protocols.types import ResourceDefinition
from dcc_mcp_core.protocols.types import ResourceTemplateDefinition
from dcc_mcp_core.protocols.types import ToolAnnotations
from dcc_mcp_core.protocols.types import ToolDefinition


class MCPAdapter:
    """Adapter to convert dcc-mcp-core primitives to MCP protocol types.

    This class provides static methods to convert Action, Resource, and Prompt
    classes to their corresponding MCP protocol definitions.

    Example:
        ```python
        from dcc_mcp_core.protocols.adapter import MCPAdapter
        from my_actions import CreateSphereAction

        # Convert an Action to MCP Tool definition
        tool_def = MCPAdapter.action_to_tool(CreateSphereAction)
        print(tool_def.name)  # "create_sphere"
        print(tool_def.inputSchema)  # JSON Schema from InputModel
        ```

    """

    @staticmethod
    def action_to_tool(
        action_class: Type[Action],
        include_output_schema: bool = True,
        include_annotations: bool = True,
    ) -> ToolDefinition:
        """Convert an Action class to MCP Tool definition.

        Args:
            action_class: The Action class to convert
            include_output_schema: Whether to include outputSchema from OutputModel
            include_annotations: Whether to include tool annotations

        Returns:
            ToolDefinition: MCP Tool definition

        Example:
            ```python
            class CreateSphereAction(Action):
                name = "create_sphere"
                description = "Creates a sphere in the scene"

                class InputModel(Action.InputModel):
                    radius: float = Field(1.0, description="Sphere radius")

                class OutputModel(Action.OutputModel):
                    object_name: str = Field(description="Created object name")

            tool = MCPAdapter.action_to_tool(CreateSphereAction)
            # tool.name == "create_sphere"
            # tool.inputSchema contains radius field schema
            ```

        """
        # Get input schema from InputModel
        input_schema = action_class.InputModel.model_json_schema()

        # Get output schema from OutputModel if requested
        output_schema: Optional[Dict[str, Any]] = None
        if include_output_schema and hasattr(action_class, "OutputModel"):
            output_schema = action_class.OutputModel.model_json_schema()
            # Remove the 'prompt' field from output schema as it's MCP-specific metadata
            if "properties" in output_schema and "prompt" in output_schema["properties"]:
                output_schema = dict(output_schema)
                output_schema["properties"] = {k: v for k, v in output_schema["properties"].items() if k != "prompt"}
                if "required" in output_schema and "prompt" in output_schema["required"]:
                    output_schema["required"] = [r for r in output_schema["required"] if r != "prompt"]

        # Build annotations if requested
        annotations: Optional[ToolAnnotations] = None
        if include_annotations:
            annotations = ToolAnnotations(
                title=getattr(action_class, "title", None) or action_class.name,
                readOnlyHint=getattr(action_class, "read_only", None),
                destructiveHint=getattr(action_class, "destructive", None),
                idempotentHint=getattr(action_class, "idempotent", None),
                openWorldHint=getattr(action_class, "open_world", None),
            )
            # Only include annotations if at least one hint is set
            if all(
                v is None
                for v in [
                    annotations.readOnlyHint,
                    annotations.destructiveHint,
                    annotations.idempotentHint,
                    annotations.openWorldHint,
                ]
            ):
                # Keep title if it differs from name
                if annotations.title == action_class.name:
                    annotations = None

        return ToolDefinition(
            name=action_class.name,
            description=action_class.description,
            inputSchema=input_schema,
            outputSchema=output_schema,
            annotations=annotations,
        )

    @staticmethod
    def resource_to_definition(resource_class: Type[Resource]) -> ResourceDefinition:
        """Convert a Resource class to MCP Resource definition.

        Args:
            resource_class: The Resource class to convert

        Returns:
            ResourceDefinition: MCP Resource definition

        Example:
            ```python
            class SceneObjectsResource(Resource):
                uri = "scene://objects"
                name = "Scene Objects"
                description = "List of all objects in the scene"
                mime_type = "application/json"

            resource_def = MCPAdapter.resource_to_definition(SceneObjectsResource)
            # resource_def.uri == "scene://objects"
            ```

        """
        return ResourceDefinition(
            uri=resource_class.get_uri(),
            name=resource_class.name,
            description=resource_class.description,
            mimeType=resource_class.mime_type,
        )

    @staticmethod
    def resource_to_template_definition(
        resource_class: Type[Resource],
    ) -> ResourceTemplateDefinition:
        """Convert a Resource class with URI template to MCP Resource Template definition.

        Args:
            resource_class: The Resource class to convert (must have uri_template)

        Returns:
            ResourceTemplateDefinition: MCP Resource Template definition

        Raises:
            ValueError: If the resource class does not have a URI template

        """
        if not resource_class.is_template():
            raise ValueError(f"Resource class {resource_class.__name__} does not have a URI template")

        return ResourceTemplateDefinition(
            uriTemplate=resource_class.uri_template,
            name=resource_class.name,
            description=resource_class.description,
            mimeType=resource_class.mime_type,
        )

    @staticmethod
    def prompt_to_definition(prompt_class: Type[Prompt]) -> PromptDefinition:
        """Convert a Prompt class to MCP Prompt definition.

        Args:
            prompt_class: The Prompt class to convert

        Returns:
            PromptDefinition: MCP Prompt definition

        Example:
            ```python
            class ModelReviewPrompt(Prompt):
                name = "model_review"
                description = "Generate a model review prompt"

                class ArgumentsModel(Prompt.ArgumentsModel):
                    object_name: str = Field(description="Object to review")

            prompt_def = MCPAdapter.prompt_to_definition(ModelReviewPrompt)
            # prompt_def.name == "model_review"
            # prompt_def.arguments contains object_name argument
            ```

        """
        # Get arguments from ArgumentsModel schema
        arguments: Optional[List[PromptArgument]] = None
        schema = prompt_class.get_arguments_schema()
        required_args = prompt_class.get_required_arguments()

        if schema.get("properties"):
            arguments = []
            for arg_name, arg_schema in schema["properties"].items():
                arguments.append(
                    PromptArgument(
                        name=arg_name,
                        description=arg_schema.get("description", ""),
                        required=arg_name in required_args,
                    )
                )

        return PromptDefinition(
            name=prompt_class.name,
            description=prompt_class.description,
            arguments=arguments,
        )

    @staticmethod
    def parse_uri_template_params(uri_template: str) -> List[str]:
        """Extract parameter names from a URI template.

        Args:
            uri_template: URI template string (e.g., "scene://objects/{name}")

        Returns:
            List of parameter names

        Example:
            ```python
            params = MCPAdapter.parse_uri_template_params("scene://objects/{category}/{name}")
            # params == ["category", "name"]
            ```

        """
        return re.findall(r"\{(\w+)\}", uri_template)

    @staticmethod
    def match_uri_to_template(uri: str, uri_template: str) -> Optional[Dict[str, str]]:
        """Match a URI against a template and extract parameters.

        Args:
            uri: The URI to match
            uri_template: The URI template to match against

        Returns:
            Dictionary of parameter values if matched, None otherwise

        Example:
            ```python
            params = MCPAdapter.match_uri_to_template(
                "scene://objects/meshes/cube1",
                "scene://objects/{category}/{name}"
            )
            # params == {"category": "meshes", "name": "cube1"}
            ```

        """
        # Convert template to regex pattern
        pattern = re.escape(uri_template)
        pattern = re.sub(r"\\{(\w+)\\}", r"(?P<\1>[^/]+)", pattern)
        pattern = f"^{pattern}$"

        match = re.match(pattern, uri)
        if match:
            return match.groupdict()
        return None
