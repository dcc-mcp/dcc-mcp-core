"""MCP Type definitions.

This module defines Pydantic models that correspond to MCP protocol types.
These are used for serialization and validation when converting dcc-mcp-core
primitives to MCP protocol format.

Reference: https://modelcontextprotocol.io/specification/2025-11-25
"""

# Import built-in modules
from typing import Any
from typing import Dict
from typing import List
from typing import Optional

# Import third-party modules
from pydantic import BaseModel
from pydantic import ConfigDict
from pydantic import Field


class ToolAnnotations(BaseModel):
    """Annotations for MCP Tool behavior hints.

    These annotations provide hints to clients about tool behavior,
    but clients should not rely on them for security or correctness.

    Attributes:
        title: Human-readable title for the tool
        readOnlyHint: If True, the tool does not modify its environment
        destructiveHint: If True, the tool may perform destructive updates
        idempotentHint: If True, repeated calls with same args have no additional effect
        openWorldHint: If True, tool interacts with external entities

    """

    title: Optional[str] = Field(None, description="Human-readable title for the tool")
    readOnlyHint: Optional[bool] = Field(None, description="If True, the tool does not modify its environment")
    destructiveHint: Optional[bool] = Field(None, description="If True, the tool may perform destructive updates")
    idempotentHint: Optional[bool] = Field(
        None, description="If True, repeated calls with same args have no additional effect"
    )
    openWorldHint: Optional[bool] = Field(None, description="If True, tool interacts with external entities")

    model_config = ConfigDict(extra="allow")


class ToolDefinition(BaseModel):
    """MCP Tool definition schema.

    Represents a tool that can be called by the client.
    Tools enable LLMs to perform actions through the server.

    Attributes:
        name: Unique identifier for the tool
        description: Human-readable description of what the tool does
        inputSchema: JSON Schema defining expected parameters
        outputSchema: Optional JSON Schema for structured output validation
        annotations: Optional behavior hints for clients

    Example:
        ```python
        tool = ToolDefinition(
            name="create_sphere",
            description="Creates a polygon sphere in Maya",
            inputSchema={
                "type": "object",
                "properties": {
                    "radius": {"type": "number", "description": "Sphere radius"},
                    "name": {"type": "string", "description": "Object name"}
                },
                "required": ["radius"]
            },
            outputSchema={
                "type": "object",
                "properties": {
                    "object_name": {"type": "string"},
                    "position": {"type": "array", "items": {"type": "number"}}
                }
            },
            annotations=ToolAnnotations(readOnlyHint=False, destructiveHint=False)
        )
        ```

    """

    name: str = Field(description="Unique identifier for the tool")
    description: str = Field(description="Human-readable description of what the tool does")
    inputSchema: Dict[str, Any] = Field(description="JSON Schema defining expected parameters")
    outputSchema: Optional[Dict[str, Any]] = Field(None, description="JSON Schema for structured output validation")
    annotations: Optional[ToolAnnotations] = Field(None, description="Behavior hints for clients")

    model_config = ConfigDict(
        json_schema_extra={
            "examples": [
                {
                    "name": "create_sphere",
                    "description": "Creates a polygon sphere in the scene",
                    "inputSchema": {
                        "type": "object",
                        "properties": {
                            "radius": {"type": "number", "default": 1.0},
                            "name": {"type": "string"},
                        },
                        "required": [],
                    },
                    "annotations": {"readOnlyHint": False},
                }
            ]
        }
    )


class ResourceDefinition(BaseModel):
    """MCP Resource definition schema.

    Represents a resource that can be read by the client.
    Resources provide data to LLMs without side effects.

    Attributes:
        uri: Unique identifier for the resource (URI format)
        name: Human-readable name for the resource
        description: Description of what the resource provides
        mimeType: MIME type of the resource content

    Example:
        ```python
        resource = ResourceDefinition(
            uri="scene://objects",
            name="Scene Objects",
            description="List of all objects in the current scene",
            mimeType="application/json"
        )
        ```

    """

    uri: str = Field(description="Unique identifier for the resource (URI format)")
    name: str = Field(description="Human-readable name for the resource")
    description: str = Field(description="Description of what the resource provides")
    mimeType: str = Field("text/plain", description="MIME type of the resource content")

    model_config = ConfigDict(
        json_schema_extra={
            "examples": [
                {
                    "uri": "scene://objects",
                    "name": "Scene Objects",
                    "description": "List of all objects in the current scene",
                    "mimeType": "application/json",
                }
            ]
        }
    )


class ResourceTemplateDefinition(BaseModel):
    """MCP Resource Template definition schema.

    Represents a template for dynamic resources with URI parameters.

    Attributes:
        uriTemplate: URI template with parameters (e.g., "scene://objects/{name}")
        name: Human-readable name for the resource template
        description: Description of what the resource provides
        mimeType: MIME type of the resource content

    """

    uriTemplate: str = Field(description="URI template with parameters")
    name: str = Field(description="Human-readable name for the resource template")
    description: str = Field(description="Description of what the resource provides")
    mimeType: str = Field("text/plain", description="MIME type of the resource content")


class PromptArgument(BaseModel):
    """MCP Prompt argument definition.

    Attributes:
        name: Name of the argument
        description: Description of the argument
        required: Whether the argument is required

    """

    name: str = Field(description="Name of the argument")
    description: str = Field(description="Description of the argument")
    required: bool = Field(False, description="Whether the argument is required")


class PromptDefinition(BaseModel):
    """MCP Prompt definition schema.

    Represents a prompt template that can be used by the client.
    Prompts provide reusable templates for LLM interactions.

    Attributes:
        name: Unique identifier for the prompt
        description: Human-readable description of the prompt
        arguments: List of arguments the prompt accepts

    Example:
        ```python
        prompt = PromptDefinition(
            name="code_review",
            description="Generate a code review prompt",
            arguments=[
                PromptArgument(name="code", description="Code to review", required=True),
                PromptArgument(name="language", description="Programming language", required=False)
            ]
        )
        ```

    """

    name: str = Field(description="Unique identifier for the prompt")
    description: str = Field(description="Human-readable description of the prompt")
    arguments: Optional[List[PromptArgument]] = Field(None, description="List of arguments the prompt accepts")

    model_config = ConfigDict(
        json_schema_extra={
            "examples": [
                {
                    "name": "model_review",
                    "description": "Generate a prompt for reviewing a 3D model",
                    "arguments": [
                        {"name": "object_name", "description": "Name of the object to review", "required": True}
                    ],
                }
            ]
        }
    )
