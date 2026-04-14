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
    # Bridge fields (for non-Python DCCs like Photoshop, ZBrush)
    has_embedded_python=True,
    bridge_kind=None,        # "http", "websocket", "named_pipe", or None
    bridge_endpoint=None,    # URL or socket path for bridge connection
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

### BridgeKind

`BridgeKind` describes how a DCC communicates when it does **not** have an embedded Python interpreter (e.g. Photoshop via UXP WebSocket, ZBrush via HTTP REST):

| Variant | Python string | Description |
|---------|---------------|-------------|
| `Http` | `"http"` | HTTP REST bridge (e.g. ZBrush 2024+) |
| `WebSocket` | `"websocket"` | WebSocket JSON-RPC bridge (e.g. Photoshop UXP) |
| `NamedPipe` | `"named_pipe"` | Named pipe bridge (e.g. 3ds Max COM) |
| `Custom(String)` | custom string | Custom bridge protocol |

In Python, `DccCapabilities.bridge_kind` is exposed as `Optional[str]` — use the string values above.

**Factory methods** on `DccCapabilities`:
- `DccCapabilities()` — Standard Python-embedded DCC (`has_embedded_python=True`)
- Set `bridge_kind="http"` + `bridge_endpoint=...` for HTTP bridge DCCs
- Set `bridge_kind="websocket"` + `bridge_endpoint=...` for WebSocket bridge DCCs

**New DCC adapter projects** (in development):
- [dcc-mcp-unreal](https://github.com/loonghao/dcc-mcp-unreal) — Unreal Engine (Python embedded)
- [dcc-mcp-photoshop](https://github.com/loonghao/dcc-mcp-photoshop) — Photoshop (WebSocket bridge via UXP)
- [dcc-mcp-zbrush](https://github.com/loonghao/dcc-mcp-zbrush) — ZBrush (HTTP REST bridge)

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

---

## DCC Adapter Traits

DCC-MCP-Core provides a set of **adapter traits** that DCC integration packages implement to connect their application to the MCP ecosystem. Client code uses these traits through the `DccAdapter` interface without knowing which DCC is running.

### Core Sub-traits

```
DccAdapter
  ├── DccConnection      — connect / disconnect / health_check
  ├── DccScriptEngine    — execute_script / supported_languages
  ├── DccSceneInfo       — get_scene_info / list_objects / get_selection
  └── DccSnapshot        — capture_viewport
```

Access a sub-trait via the corresponding accessor method:

```python
adapter = get_adapter()   # returns a DccAdapter implementation

# Script execution
if engine := adapter.as_script_engine():
    result = engine.execute_script("import maya.cmds; print(cmds.ls())", "python")

# Scene info
if scene_info := adapter.as_scene_info():
    info = scene_info.get_scene_info()
    print(f"Scene: {info.file_path}  objects: {info.statistics.object_count}")

# Viewport capture
if snapshot := adapter.as_snapshot():
    capture = snapshot.capture_viewport(None, 1920, 1080, "png")
    with open("viewport.png", "wb") as f:
        f.write(capture.data)
```

### Cross-DCC Protocol Traits

Four optional traits provide **universal operations** that work across Maya, Blender, 3ds Max, Unreal Engine, Unity, Photoshop, and Figma. Always check if the adapter supports them via `DccCapabilities` before calling:

```python
caps = adapter.capabilities()

if caps.scene_manager:
    mgr = adapter.as_scene_manager()
    objects = mgr.list_objects("mesh")          # all mesh objects
    mgr.set_visibility("pCube1", False)         # hide an object
    saved = mgr.save_file(None)                 # save in place

if caps.transform:
    xform = adapter.as_transform()
    t = xform.get_transform("pCube1")
    print(f"position: {t.translate}")
    xform.set_transform("pCube1", translate=[10.0, 0.0, 0.0])

if caps.hierarchy:
    hier = adapter.as_hierarchy()
    tree = hier.get_hierarchy()                 # full scene tree
    children = hier.get_children("group1")     # immediate children

if caps.render_capture:
    rc = adapter.as_render_capture()
    output = rc.render_scene("/renders/frame001.exr", 1920, 1080)
    print(f"rendered in {output.render_time_ms}ms → {output.file_path}")
```

### Implementing a DCC Adapter

To connect a new DCC to the MCP ecosystem, implement the adapter interface in your DCC integration package. The adapter traits (`DccAdapter`, `DccConnection`, `DccScriptEngine`, etc.) are Rust traits — they are **not importable from Python**. Python-side adapters use duck typing and register themselves with the Rust runtime. Only data types (`DccInfo`, `DccCapabilities`, `ScriptLanguage`, etc.) are exported from `dcc_mcp_core`.

```python
# In your DCC integration package (e.g. dcc-mcp-maya)
# DccAdapter / DccConnection / DccScriptEngine are Rust traits — not Python imports.
# Import only the data types you need to construct return values.
from dcc_mcp_core import (
    DccInfo, DccCapabilities,
    ScriptResult, ScriptLanguage,
)

class MayaAdapter:
    """Duck-typed DccAdapter implementation for Maya."""

    def __init__(self):
        self._info = DccInfo(
            dcc_type="maya",
            version="2025",
            python_version="3.11.7",
            platform="win64",
            pid=12345,
        )

    def info(self):
        return self._info

    def capabilities(self):
        return DccCapabilities(
            script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL],
            scene_info=True,
            snapshot=True,
            scene_manager=True,
            transform=True,
            hierarchy=True,
        )

    def as_connection(self):
        return self._connection_impl   # duck-typed DccConnection

    def as_script_engine(self):
        return self._script_engine     # duck-typed DccScriptEngine

    def as_scene_info(self):
        return self._scene_info        # duck-typed DccSceneInfo

    def as_snapshot(self):
        return self._snapshot          # duck-typed DccSnapshot

    def as_scene_manager(self):
        return self._scene_manager     # duck-typed DccSceneManager

    def as_transform(self):
        return self._transform         # duck-typed DccTransform

    def as_hierarchy(self):
        return self._hierarchy         # duck-typed DccHierarchy
```

::: tip Coordinate Convention
All `DccTransform` implementations must use **right-hand Y-up world space**, Euler XYZ rotation in **degrees**, and **centimeter** units. Adapters are responsible for converting from their native coordinate system (e.g. Blender Z-up radians, Unreal centimeter Z-up).
:::

::: info Optional Sub-traits
The four cross-DCC protocol traits (`DccSceneManager`, `DccTransform`, `DccRenderCapture`, `DccHierarchy`) are **optional** — return `None` from the accessor if your DCC does not support them. Set the corresponding field in `DccCapabilities` to `False` so clients can check support before calling.
:::
