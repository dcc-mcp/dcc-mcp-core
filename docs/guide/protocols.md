# MCP Protocols

DCC-MCP-Core provides a full MCP (Model Context Protocol) abstraction layer with type definitions, abstract base classes, and adapters.

## Type Definitions

```python
from dcc_mcp_core.protocols import (
    ToolDefinition, ToolAnnotations,
    ResourceDefinition, ResourceTemplateDefinition,
    PromptDefinition, PromptArgument,
)

tool = ToolDefinition(
    name="create_sphere",
    description="Creates a sphere",
    inputSchema={"type": "object", "properties": {"radius": {"type": "number"}}},
    annotations=ToolAnnotations(readOnlyHint=False),
)
```

## Resource Base Class

```python
from dcc_mcp_core.protocols import Resource
from pydantic import Field

class SceneObjectsResource(Resource):
    uri = "scene://objects"
    name = "Scene Objects"
    description = "All objects in the current scene"
    mime_type = "application/json"
    dcc = "maya"

    def read(self, **params) -> str:
        import json
        objects = self.context["cmds"].ls(dag=True)
        return json.dumps(objects)
```

## Prompt Base Class

```python
from dcc_mcp_core.protocols import Prompt
from pydantic import Field

class ReviewPrompt(Prompt):
    name = "model_review"
    description = "Review a 3D model"

    class ArgumentsModel(Prompt.ArgumentsModel):
        object_name: str = Field(description="Object to review")

    def render(self, **kwargs) -> str:
        args = self.ArgumentsModel(**kwargs)
        return f"Review the 3D model '{args.object_name}'"
```

## MCPAdapter

Convert Action classes and protocol objects to MCP-compatible type definitions:

```python
from dcc_mcp_core.protocols import MCPAdapter

# Convert Action to Tool definition
tool_def = MCPAdapter.action_to_tool(CreateSphereAction)

# Convert Resource to definition
resource_def = MCPAdapter.resource_to_definition(SceneObjectsResource)

# Convert Prompt to definition
prompt_def = MCPAdapter.prompt_to_definition(ReviewPrompt)

# URI template helpers
params = MCPAdapter.parse_uri_template_params("scene://objects/{category}/{name}")
# ["category", "name"]

matched = MCPAdapter.match_uri_to_template(
    "scene://objects/mesh/cube1",
    "scene://objects/{category}/{name}"
)
# {"category": "mesh", "name": "cube1"}
```

## Server Protocol

Implement the full MCP server interface:

```python
from dcc_mcp_core.protocols import MCPServerProtocol

class MyMCPServer:  # implements MCPServerProtocol
    @property
    def name(self) -> str: return "my-server"
    @property
    def version(self) -> str: return "1.0.0"

    async def list_tools(self): ...
    async def call_tool(self, name, arguments): ...
    async def list_resources(self): ...
    async def read_resource(self, uri): ...
    async def list_prompts(self): ...
    async def get_prompt(self, name, arguments=None): ...

# Runtime type checking
assert isinstance(my_server, MCPServerProtocol)
```
