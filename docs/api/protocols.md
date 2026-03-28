# Protocols API

`dcc_mcp_core.protocols`

## Type Definitions

### ToolDefinition

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Tool name |
| `description` | `str` | Tool description |
| `inputSchema` | `dict` | JSON Schema for input |
| `annotations` | `ToolAnnotations` | Optional annotations |

### ResourceDefinition

| Field | Type | Description |
|-------|------|-------------|
| `uri` | `str` | Resource URI |
| `name` | `str` | Resource name |
| `description` | `str` | Description |
| `mimeType` | `str` | MIME type |

### PromptDefinition

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Prompt name |
| `description` | `str` | Description |
| `arguments` | `List[PromptArgument]` | Arguments |

## Abstract Base Classes

### Resource

```python
class Resource(ABC):
    uri: str
    name: str
    description: str
    mime_type: str
    dcc: str

    @abstractmethod
    def read(self, **params) -> str: ...
```

### Prompt

```python
class Prompt(ABC):
    name: str
    description: str

    class ArgumentsModel(BaseModel): ...

    @abstractmethod
    def render(self, **kwargs) -> str: ...
```

## MCPAdapter

| Method | Description |
|--------|-------------|
| `action_to_tool(action_class)` | Convert Action to ToolDefinition |
| `resource_to_definition(resource_class)` | Convert Resource to ResourceDefinition |
| `prompt_to_definition(prompt_class)` | Convert Prompt to PromptDefinition |
| `parse_uri_template_params(template)` | Extract params from URI template |
| `match_uri_to_template(uri, template)` | Match URI against template |

## MCPServerProtocol

```python
class MCPServerProtocol:
    name: str
    version: str

    async def list_tools() -> List[ToolDefinition]
    async def call_tool(name, arguments) -> Any
    async def list_resources() -> List[ResourceDefinition]
    async def read_resource(uri) -> str
    async def list_prompts() -> List[PromptDefinition]
    async def get_prompt(name, arguments=None) -> str
```
