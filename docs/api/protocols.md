# Protocols API

`dcc_mcp_core.protocols` types — MCP specification-compliant type definitions.

## ToolDefinition

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Tool name |
| `description` | `str` | Tool description |
| `input_schema` | `str` | JSON Schema string for input (serde: `inputSchema`) |
| `output_schema` | `Optional[str]` | JSON Schema string for output (serde: `outputSchema`) |

```python
from dcc_mcp_core import ToolDefinition

tool = ToolDefinition(
    name="create_sphere",
    description="Creates a sphere",
    input_schema='{"type": "object"}',
)
```

## ToolAnnotations

| Field | Type | Description |
|-------|------|-------------|
| `title` | `Optional[str]` | Human-readable title |
| `read_only_hint` | `Optional[bool]` | serde: `readOnlyHint` |
| `destructive_hint` | `Optional[bool]` | serde: `destructiveHint` |
| `idempotent_hint` | `Optional[bool]` | serde: `idempotentHint` |
| `open_world_hint` | `Optional[bool]` | serde: `openWorldHint` |

```python
from dcc_mcp_core import ToolAnnotations

ann = ToolAnnotations(read_only_hint=True)
```

## ResourceDefinition

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `uri` | `str` | — | Resource URI |
| `name` | `str` | — | Resource name |
| `description` | `str` | — | Description |
| `mime_type` | `str` | `"text/plain"` | MIME type (serde: `mimeType`) |

```python
from dcc_mcp_core import ResourceDefinition

res = ResourceDefinition(uri="scene://objects", name="Objects", description="Scene objects")
```

## ResourceTemplateDefinition

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `uri_template` | `str` | — | URI template (serde: `uriTemplate`) |
| `name` | `str` | — | Template name |
| `description` | `str` | — | Description |
| `mime_type` | `str` | `"text/plain"` | MIME type (serde: `mimeType`) |

```python
from dcc_mcp_core import ResourceTemplateDefinition

tmpl = ResourceTemplateDefinition(
    uri_template="scene://objects/{name}",
    name="Object",
    description="A scene object",
)
```

## PromptArgument

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | — | Argument name |
| `description` | `str` | — | Description |
| `required` | `bool` | `False` | Whether required |

```python
from dcc_mcp_core import PromptArgument

arg = PromptArgument(name="object_name", description="Object to review", required=True)
```

## PromptDefinition

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Prompt name |
| `description` | `str` | Description |

```python
from dcc_mcp_core import PromptDefinition

prompt = PromptDefinition(name="review", description="Review a model")
```
