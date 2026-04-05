# MCP 协议

DCC-MCP-Core 提供完整的 MCP（模型上下文协议）抽象层，包含类型定义、抽象基类和适配器。

## 类型定义

```python
from dcc_mcp_core.protocols import (
    ToolDefinition, ToolAnnotations,
    ResourceDefinition, ResourceTemplateDefinition,
    PromptDefinition, PromptArgument,
)

tool = ToolDefinition(
    name="create_sphere",
    description="创建球体",
    inputSchema={"type": "object", "properties": {"radius": {"type": "number"}}},
    annotations=ToolAnnotations(readOnlyHint=False),
)
```

## Resource 基类

```python
from dcc_mcp_core.protocols import Resource
from pydantic import Field

class SceneObjectsResource(Resource):
    uri = "scene://objects"
    name = "Scene Objects"
    description = "当前场景中的所有对象"
    mime_type = "application/json"
    dcc = "maya"

    def read(self, **params) -> str:
        import json
        objects = self.context["cmds"].ls(dag=True)
        return json.dumps(objects)
```

## Prompt 基类

```python
from dcc_mcp_core.protocols import Prompt
from pydantic import Field

class ReviewPrompt(Prompt):
    name = "model_review"
    description = "审查一个 3D 模型"

    class ArgumentsModel(Prompt.ArgumentsModel):
        object_name: str = Field(description="要审查的对象")

    def render(self, **kwargs) -> str:
        args = self.ArgumentsModel(**kwargs)
        return f"审查 3D 模型 '{args.object_name}'"
```

## MCPAdapter

将 Action 类和协议对象转换为 MCP 兼容的类型定义：

```python
from dcc_mcp_core.protocols import MCPAdapter

# Action 转 Tool 定义
tool_def = MCPAdapter.action_to_tool(CreateSphereAction)

# Resource 转定义
resource_def = MCPAdapter.resource_to_definition(SceneObjectsResource)

# Prompt 转定义
prompt_def = MCPAdapter.prompt_to_definition(ReviewPrompt)

# URI 模板辅助函数
params = MCPAdapter.parse_uri_template_params("scene://objects/{category}/{name}")
# ["category", "name"]

matched = MCPAdapter.match_uri_to_template(
    "scene://objects/mesh/cube1",
    "scene://objects/{category}/{name}"
)
# {"category": "mesh", "name": "cube1"}
```

## Server Protocol

实现完整的 MCP 服务器接口：

```python
from dcc_mcp_core.protocols import MCPServerProtocol

class MyMCPServer:  # 实现 MCPServerProtocol
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

# 运行时类型检查
assert isinstance(my_server, MCPServerProtocol)
```
