# 协议 API

`dcc_mcp_core.protocols` 类型 — 符合 MCP 规范的类型定义。

## ToolDefinition

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 工具名称 |
| `description` | `str` | 工具描述 |
| `input_schema` | `str` | 输入的 JSON Schema 字符串（serde: `inputSchema`） |
| `output_schema` | `Optional[str]` | 输出的 JSON Schema 字符串（serde: `outputSchema`） |

```python
from dcc_mcp_core import ToolDefinition

tool = ToolDefinition(
    name="create_sphere",
    description="Creates a sphere",
    input_schema='{"type": "object"}',
)
```

## ToolAnnotations

| 字段 | 类型 | 说明 |
|------|------|------|
| `title` | `Optional[str]` | 人类可读标题 |
| `read_only_hint` | `Optional[bool]` | serde: `readOnlyHint` |
| `destructive_hint` | `Optional[bool]` | serde: `destructiveHint` |
| `idempotent_hint` | `Optional[bool]` | serde: `idempotentHint` |
| `open_world_hint` | `Optional[bool]` | serde: `openWorldHint` |

```python
from dcc_mcp_core import ToolAnnotations

ann = ToolAnnotations(read_only_hint=True)
```

## ResourceDefinition

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `uri` | `str` | — | 资源 URI |
| `name` | `str` | — | 资源名称 |
| `description` | `str` | — | 描述 |
| `mime_type` | `str` | `"text/plain"` | MIME 类型（serde: `mimeType`） |

```python
from dcc_mcp_core import ResourceDefinition

res = ResourceDefinition(uri="scene://objects", name="Objects", description="场景对象")
```

## ResourceTemplateDefinition

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `uri_template` | `str` | — | URI 模板（serde: `uriTemplate`） |
| `name` | `str` | — | 模板名称 |
| `description` | `str` | — | 描述 |
| `mime_type` | `str` | `"text/plain"` | MIME 类型（serde: `mimeType`） |

```python
from dcc_mcp_core import ResourceTemplateDefinition

tmpl = ResourceTemplateDefinition(
    uri_template="scene://objects/{name}",
    name="Object",
    description="场景对象",
)
```

## PromptArgument

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | — | 参数名称 |
| `description` | `str` | — | 描述 |
| `required` | `bool` | `False` | 是否必需 |

```python
from dcc_mcp_core import PromptArgument

arg = PromptArgument(name="object_name", description="要审查的对象", required=True)
```

## PromptDefinition

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 提示名称 |
| `description` | `str` | 描述 |

```python
from dcc_mcp_core import PromptDefinition

prompt = PromptDefinition(name="review", description="审查一个模型")
```
