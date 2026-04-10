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

---

## DCC Adapter Traits

DCC integration packages implement these traits to expose their application to the MCP ecosystem. All methods are synchronous.

### Architecture Overview

```
DccAdapter              — Top-level trait
  ├── DccConnection         — Connection lifecycle
  ├── DccScriptEngine       — Script execution (Python / MEL / MaxScript / …)
  ├── DccSceneInfo          — Scene inspection
  └── DccSnapshot           — Viewport capture

Cross-DCC Protocol Traits (universal, optional)
  ├── DccSceneManager       — Scene/file management, selection, visibility
  ├── DccTransform          — Object TRS transforms and bounding boxes
  ├── DccRenderCapture      — Viewport capture and scene rendering
  └── DccHierarchy          — Parent/child hierarchy and grouping
```

### DccAdapter

Top-level trait. Implement in your DCC integration package.

| Method | Returns | Description |
|--------|---------|-------------|
| `info()` | `DccInfo` | Static application info (type, version, pid, platform) |
| `capabilities()` | `DccCapabilities` | Feature flags — advertise which sub-traits are available |
| `as_connection()` | `DccConnection \| None` | Connection lifecycle interface |
| `as_script_engine()` | `DccScriptEngine \| None` | Script execution interface |
| `as_scene_info()` | `DccSceneInfo \| None` | Scene info query interface |
| `as_snapshot()` | `DccSnapshot \| None` | Screenshot/capture interface |
| `as_scene_manager()` | `DccSceneManager \| None` | Universal scene management (optional) |
| `as_transform()` | `DccTransform \| None` | Universal object TRS (optional) |
| `as_render_capture()` | `DccRenderCapture \| None` | Render/capture interface (optional) |
| `as_hierarchy()` | `DccHierarchy \| None` | Scene hierarchy interface (optional) |

### DccConnection

| Method | Returns | Description |
|--------|---------|-------------|
| `connect()` | `None` | Establish connection to the DCC |
| `disconnect()` | `None` | Disconnect from the DCC |
| `is_connected()` | `bool` | Whether the connection is alive |
| `health_check()` | `int` | Round-trip ping in milliseconds |

### DccScriptEngine

| Method | Returns | Description |
|--------|---------|-------------|
| `execute_script(code, language, timeout_ms)` | `ScriptResult` | Run a script inside the DCC |
| `supported_languages()` | `list[ScriptLanguage]` | Languages this DCC supports |

### DccSceneInfo

| Method | Returns | Description |
|--------|---------|-------------|
| `get_scene_info()` | `SceneInfo` | Info about the currently open scene |
| `list_objects()` | `list[tuple[str, str]]` | `(name, type)` pairs for all scene objects |
| `get_selection()` | `list[str]` | Names of currently selected objects |

### DccSnapshot

| Method | Returns | Description |
|--------|---------|-------------|
| `capture_viewport(viewport, width, height, format)` | `CaptureResult` | Capture a viewport as PNG / JPEG / WebP |

### DccSceneManager

Universal scene and file management. Supported across Maya, Blender, 3dsMax, Unreal, Unity, Photoshop, Figma.

| Method | Returns | Description |
|--------|---------|-------------|
| `get_scene_info()` | `SceneInfo` | Metadata for the current scene/document |
| `list_objects(object_type)` | `list[SceneObject]` | All objects; filter by type or `None` for all |
| `new_scene(save_prompt)` | `SceneInfo` | Create a new empty scene |
| `open_file(file_path, force)` | `SceneInfo` | Open scene from disk |
| `save_file(file_path)` | `str` | Save scene; `None` = save in place |
| `export_file(file_path, format, selection_only)` | `str` | Export to FBX / OBJ / USD / PNG etc. |
| `get_selection()` | `list[str]` | Currently selected object names |
| `set_selection(object_names)` | `list[str]` | Replace selection |
| `select_by_type(object_type)` | `list[str]` | Select all objects of a given type |
| `set_visibility(object_name, visible)` | `bool` | Toggle object/layer visibility |

### DccTransform

Universal TRS interface. Coordinate convention: right-hand Y-up world space, Euler XYZ in degrees, centimeter units.

| Method | Returns | Description |
|--------|---------|-------------|
| `get_transform(object_name)` | `ObjectTransform` | World-space TRS |
| `set_transform(object_name, translate, rotate, scale)` | `ObjectTransform` | Update TRS; pass `None` to leave a component unchanged |
| `get_bounding_box(object_name)` | `BoundingBox` | World-space AABB |
| `rename_object(old_name, new_name)` | `str` | Rename; returns new long name |

### DccRenderCapture

Viewport screenshot and scene render output.

| Method | Returns | Description |
|--------|---------|-------------|
| `capture_viewport(viewport, width, height, format)` | `CaptureResult` | Screenshot of active/named viewport |
| `render_scene(output_path, width, height, renderer)` | `RenderOutput` | Full render to disk |
| `get_render_settings()` | `dict[str, str]` | Current render settings |
| `set_render_settings(settings)` | `None` | Update one or more render settings |

### DccHierarchy

Parent-child object graph — Maya DAG, Blender collections, UE level, Unity scene graph, Photoshop layer groups, Figma frames.

| Method | Returns | Description |
|--------|---------|-------------|
| `get_hierarchy()` | `list[SceneNode]` | Full scene tree (root nodes with nested children) |
| `get_children(object_name)` | `list[SceneObject]` | Immediate children; `None` = scene root |
| `get_parent(object_name)` | `str \| None` | Parent name; `None` when at scene root |
| `group_objects(object_names, group_name, parent)` | `SceneObject` | Group under a new container |
| `ungroup(group_name)` | `list[str]` | Dissolve group; children move to group's parent |
| `reparent(object_name, new_parent, preserve_world_transform)` | `SceneObject` | Change parent |

---

## Cross-DCC Data Models

### ObjectTransform

| Field | Type | Description |
|-------|------|-------------|
| `translate` | `[float, float, float]` | World XYZ in centimeters |
| `rotate` | `[float, float, float]` | Euler XYZ in degrees |
| `scale` | `[float, float, float]` | Non-uniform scale (sx, sy, sz) |

```python
from dcc_mcp_core import ObjectTransform

t = ObjectTransform(
    translate=[10.0, 0.0, 5.0],
    rotate=[0.0, 45.0, 0.0],
    scale=[1.0, 1.0, 1.0],
)
identity = ObjectTransform.identity()  # origin, no rotation, scale=1
```

### BoundingBox

| Field | Type | Description |
|-------|------|-------------|
| `min` | `[float, float, float]` | Minimum corner in world space (cm) |
| `max` | `[float, float, float]` | Maximum corner in world space (cm) |

```python
from dcc_mcp_core import BoundingBox

bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
bb.center()  # [0.0, 1.0, 0.0]
bb.size()    # [2.0, 2.0, 2.0]
```

### SceneObject

Lightweight description of any scene object, layer, or actor.

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Short leaf name (e.g. `pCube1`) |
| `long_name` | `str` | Full path / unique ID (e.g. `\|group1\|pCube1`) |
| `object_type` | `str` | Type string (`mesh`, `light`, `camera`, …) |
| `parent` | `str \| None` | Parent long name; `None` = scene root |
| `visible` | `bool` | Whether the object is visible |
| `metadata` | `dict[str, str]` | Arbitrary extra data |

### SceneNode

| Field | Type | Description |
|-------|------|-------------|
| `object` | `SceneObject` | The object at this node |
| `children` | `list[SceneNode]` | Immediate children (nested recursively) |

### FrameRange

| Field | Type | Description |
|-------|------|-------------|
| `start` | `float` | First frame (inclusive) |
| `end` | `float` | Last frame (inclusive) |
| `fps` | `float` | Frames per second |
| `current` | `float` | Currently active frame |

### RenderOutput

Result of `DccRenderCapture.render_scene()`.

| Field | Type | Description |
|-------|------|-------------|
| `file_path` | `str` | Absolute path to the rendered image |
| `width` | `int` | Image width in pixels |
| `height` | `int` | Image height in pixels |
| `format` | `str` | File format (`png`, `exr`, `jpg`) |
| `render_time_ms` | `int` | Render duration in milliseconds |
