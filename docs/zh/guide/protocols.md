# MCP 协议

DCC-MCP-Core 提供 MCP（模型上下文协议）类型定义，用于 Tools、Resources 和 Prompts。所有类型直接从 `dcc_mcp_core` 导出。

## ToolDefinition

定义一个 MCP tool：

```python
import json
from dcc_mcp_core import ToolDefinition, ToolAnnotations

tool = ToolDefinition(
    name="create_sphere",
    description="在 DCC 场景中创建多边形球体",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {
            "radius": {"type": "number", "minimum": 0.1},
            "segments": {"type": "integer", "minimum": 4, "default": 16},
        }
    }),
    output_schema=json.dumps({
        "type": "object",
        "properties": {
            "object_name": {"type": "string"},
            "radius": {"type": "number"},
        }
    }),
    annotations=ToolAnnotations(
        title="Create Sphere",
        read_only_hint=False,
        destructive_hint=False,
        idempotent_hint=False,
    ),
)
```

### ToolAnnotations

LLM 的可选行为提示：

| 字段 | 类型 | 说明 |
|------|------|------|
| `title` | `str?` | 可读的显示名称 |
| `read_only_hint` | `bool?` | True 表示不修改状态 |
| `destructive_hint` | `bool?` | True 表示可能具有破坏性 |
| `idempotent_hint` | `bool?` | True 表示重复调用安全 |
| `open_world_hint` | `bool?` | True 表示访问外部世界 |

## ResourceDefinition

定义一个 MCP resource：

```python
from dcc_mcp_core import ResourceDefinition, ResourceAnnotations

resource = ResourceDefinition(
    uri="scene://objects",
    name="Scene Objects",
    description="当前 DCC 场景中的所有对象",
    mime_type="application/json",
    annotations=ResourceAnnotations(
        audience=["agent"],
        priority=0.8,
    ),
)
```

## ResourceTemplateDefinition

参数化资源的 URI 模板：

```python
from dcc_mcp_core import ResourceTemplateDefinition

template = ResourceTemplateDefinition(
    uri_template="scene://objects/{category}/{name}",
    name="Scoped Object",
    description="按类别和名称访问对象",
    mime_type="application/json",
)
```

## PromptDefinition

定义一个 MCP prompt：

```python
from dcc_mcp_core import PromptDefinition, PromptArgument

prompt = PromptDefinition(
    name="review_scene",
    description="Review the current DCC scene state",
    arguments=[
        PromptArgument(
            name="focus_area",
            description="Area to focus review on",
            required=False,
        ),
    ],
)
```

## DCC 信息类型

```python
from dcc_mcp_core import (
    DccInfo, DccCapabilities, DccError, DccErrorCode,
    ScriptLanguage, ScriptResult, SceneInfo, SceneStatistics,
)

# DCC 应用信息
info = DccInfo(
    dcc_type="maya",
    version="2025",
    platform="win64",
    pid=12345,
    python_version="3.11.7",
)

# DCC 能力
caps = DccCapabilities(
    script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL],
    scene_info=True,
    snapshot=True,
    undo_redo=True,
    progress_reporting=True,
    file_operations=True,
    selection=True,
)

# DCC 错误
err = DccError(
    code=DccErrorCode.SCRIPT_ERROR,
    message="Maya command failed",
    details="polySphere: object 'pSphere1' already exists",
    recoverable=True,
)

# 脚本执行结果
result = ScriptResult(
    success=False,
    execution_time_ms=150,
    output=None,
    error="Name conflict",
)

# 场景信息
stats = SceneStatistics(
    object_count=42,
    vertex_count=100000,
    polygon_count=5000,
)
scene = SceneInfo(
    file_path="/project/scene.usda",
    name="scene",
    modified=True,
    format="usda",
    frame_range=(1.0, 240.0),
    current_frame=1.0,
    fps=24.0,
    up_axis="Y",
    units="cm",
    statistics=stats,
)
```

### DccErrorCode 枚举

| 值 | 说明 |
|----|------|
| `CONNECTION_FAILED` | 无法连接到 DCC |
| `TIMEOUT` | 操作超时 |
| `SCRIPT_ERROR` | 脚本执行错误 |
| `NOT_RESPONDING` | DCC 无响应 |
| `UNSUPPORTED` | 操作不支持 |
| `PERMISSION_DENIED` | 权限被拒绝 |
| `INVALID_INPUT` | 输入参数无效 |
| `SCENE_ERROR` | 场景操作错误 |
| `INTERNAL` | 内部错误 |

### ScriptLanguage 枚举

| 值 | 说明 |
|----|------|
| `PYTHON` | Python 脚本 |
| `MEL` | MEL 脚本（Maya） |
| `MAXSCRIPT` | 3ds Max MaxScript |
| `HSCRIPT` | Houdini 脚本 |
| `VEX` | VEX 代码片段 |
| `LUA` | Lua 脚本 |
| `CSHARP` | C# 脚本 |
| `BLUEPRINT` | 可视化脚本 |
