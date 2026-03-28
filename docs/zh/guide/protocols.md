# MCP 协议

DCC-MCP-Core 提供完整的 MCP（模型上下文协议）抽象层。

## 类型定义

```python
from dcc_mcp_core.protocols import ToolDefinition, ResourceDefinition, PromptDefinition

tool = ToolDefinition(
    name="create_sphere",
    description="创建球体",
    inputSchema={"type": "object", "properties": {"radius": {"type": "number"}}},
)
```

## Resource 基类

```python
from dcc_mcp_core.protocols import Resource

class SceneObjectsResource(Resource):
    uri = "scene://objects"
    name = "Scene Objects"
    description = "当前场景中的所有对象"
    mime_type = "application/json"
    dcc = "maya"

    def read(self, **params) -> str:
        import json
        return json.dumps(self.context["cmds"].ls(dag=True))
```

## MCPAdapter

```python
from dcc_mcp_core.protocols import MCPAdapter

tool_def = MCPAdapter.action_to_tool(CreateSphereAction)
resource_def = MCPAdapter.resource_to_definition(SceneObjectsResource)
```

## Server Protocol

```python
from dcc_mcp_core.protocols import MCPServerProtocol

class MyMCPServer:  # 实现 MCPServerProtocol
    async def list_tools(self): ...
    async def call_tool(self, name, arguments): ...
    async def list_resources(self): ...
    async def read_resource(self, uri): ...
    async def list_prompts(self): ...
    async def get_prompt(self, name, arguments=None): ...
```
