# MCP Protocols

DCC-MCP-Core provides MCP (Model Context Protocol) type definitions for Tools, Resources, and Prompts. All types are exported directly from `dcc_mcp_core`.

## ToolDefinition

Define an MCP tool:

```python
import json
from dcc_mcp_core import ToolDefinition, ToolAnnotations

tool = ToolDefinition(
    name="create_sphere",
    description="Create a polygon sphere in the DCC scene",
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

Optional behavioral hints for the LLM:

| Field | Type | Description |
|-------|------|-------------|
| `title` | `str?` | Human-readable display name |
| `read_only_hint` | `bool?` | True if tool does not modify state |
| `destructive_hint` | `bool?` | True if tool may be destructive |
| `idempotent_hint` | `bool?` | True if repeated calls are safe |
| `open_world_hint` | `bool?` | True if tool accesses external world |

## ResourceDefinition

Define an MCP resource:

```python
from dcc_mcp_core import ResourceDefinition, ResourceAnnotations

resource = ResourceDefinition(
    uri="scene://objects",
    name="Scene Objects",
    description="All objects in the current DCC scene",
    mime_type="application/json",
    annotations=ResourceAnnotations(
        audience=["agent"],
        priority=0.8,
    ),
)
```

## ResourceTemplateDefinition

URI template for parameterized resources:

```python
from dcc_mcp_core import ResourceTemplateDefinition

template = ResourceTemplateDefinition(
    uri_template="scene://objects/{category}/{name}",
    name="Scoped Object",
    description="Object by category and name",
    mime_type="application/json",
)
```

## PromptDefinition

Define an MCP prompt:

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

## DCC Info Types

```python
from dcc_mcp_core import (
    DccInfo, DccCapabilities, DccError, DccErrorCode,
    ScriptLanguage, ScriptResult, SceneInfo, SceneStatistics,
)

# DCC application info
info = DccInfo(
    dcc_type="maya",
    version="2025",
    platform="win64",
    pid=12345,
    python_version="3.11.7",
)

# DCC capabilities
caps = DccCapabilities(
    script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL],
    scene_info=True,
    snapshot=True,
    undo_redo=True,
    progress_reporting=True,
    file_operations=True,
    selection=True,
)

# DCC error
err = DccError(
    code=DccErrorCode.SCRIPT_ERROR,
    message="Maya command failed",
    details="polySphere: object 'pSphere1' already exists",
    recoverable=True,
)

# Script execution result
result = ScriptResult(
    success=False,
    execution_time_ms=150,
    output=None,
    error="Name conflict",
)

# Scene info
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

### DccErrorCode Enum

| Value | Description |
|-------|-------------|
| `CONNECTION_FAILED` | Cannot connect to DCC |
| `TIMEOUT` | Operation timed out |
| `SCRIPT_ERROR` | Script execution error |
| `NOT_RESPONDING` | DCC is not responding |
| `UNSUPPORTED` | Operation not supported |
| `PERMISSION_DENIED` | Permission denied |
| `INVALID_INPUT` | Invalid input parameters |
| `SCENE_ERROR` | Scene operation error |
| `INTERNAL` | Internal error |

### ScriptLanguage Enum

| Value | Description |
|-------|-------------|
| `PYTHON` | Python scripts |
| `MEL` | MEL scripts (Maya) |
| `MAXSCRIPT` | 3ds Max MaxScript |
| `HSCRIPT` | Houdini scripts |
| `VEX` | VEX snippets |
| `LUA` | Lua scripts |
| `CSHARP` | C# scripts |
| `BLUEPRINT` | Visual scripting |
