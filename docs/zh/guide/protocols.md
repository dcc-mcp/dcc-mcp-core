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

---

## DCC Adapter Traits

DCC-MCP-Core 提供一套 **adapter traits**，由各 DCC 集成包实现，将应用接入 MCP 生态系统。客户端代码通过 `DccAdapter` 接口使用这些 trait，无需关心底层具体是哪款 DCC。

### 核心子 Traits

```
DccAdapter
  ├── DccConnection      — connect / disconnect / health_check
  ├── DccScriptEngine    — execute_script / supported_languages
  ├── DccSceneInfo       — get_scene_info / list_objects / get_selection
  └── DccSnapshot        — capture_viewport
```

通过对应的访问器方法获取子 trait：

```python
adapter = get_adapter()   # 返回 DccAdapter 实现

# 脚本执行
if engine := adapter.as_script_engine():
    result = engine.execute_script("import maya.cmds; print(cmds.ls())", "python")

# 场景信息
if scene_info := adapter.as_scene_info():
    info = scene_info.get_scene_info()
    print(f"场景: {info.file_path}  对象数: {info.statistics.object_count}")

# 视口截图
if snapshot := adapter.as_snapshot():
    capture = snapshot.capture_viewport(None, 1920, 1080, "png")
    with open("viewport.png", "wb") as f:
        f.write(capture.data)
```

### 跨 DCC 协议 Traits

四个可选 trait 提供**通用操作**，可在 Maya、Blender、3ds Max、Unreal Engine、Unity、Photoshop、Figma 等 DCC 间通用。调用前请先通过 `DccCapabilities` 检查适配器是否支持：

```python
caps = adapter.capabilities()

if caps.scene_manager:
    mgr = adapter.as_scene_manager()
    objects = mgr.list_objects("mesh")          # 所有网格对象
    mgr.set_visibility("pCube1", False)         # 隐藏对象
    saved = mgr.save_file(None)                 # 原位保存

if caps.transform:
    xform = adapter.as_transform()
    t = xform.get_transform("pCube1")
    print(f"位置: {t.translate}")
    xform.set_transform("pCube1", translate=[10.0, 0.0, 0.0])

if caps.hierarchy:
    hier = adapter.as_hierarchy()
    tree = hier.get_hierarchy()                 # 完整场景树
    children = hier.get_children("group1")     # 直接子节点

if caps.render_capture:
    rc = adapter.as_render_capture()
    output = rc.render_scene("/renders/frame001.exr", 1920, 1080)
    print(f"渲染完成，耗时 {output.render_time_ms}ms → {output.file_path}")
```

### 实现 DCC Adapter

要将新 DCC 接入 MCP 生态，需在你的 DCC 集成包中实现适配器接口。适配器 trait（`DccAdapter`、`DccConnection`、`DccScriptEngine` 等）是 Rust trait——**不可从 Python 导入**。Python 侧适配器使用鸭子类型（duck typing）实现，并向 Rust 运行时注册。只有数据类型（`DccInfo`、`DccCapabilities`、`ScriptLanguage` 等）才从 `dcc_mcp_core` 导出。

```python
# 在你的 DCC 集成包中（如 dcc-mcp-maya）
# DccAdapter / DccConnection / DccScriptEngine 是 Rust trait，不可从 Python 导入。
# 只需导入构造返回值所需的数据类型。
from dcc_mcp_core import (
    DccInfo, DccCapabilities,
    ScriptResult, ScriptLanguage,
)

class MayaAdapter:
    """Maya 的鸭子类型 DccAdapter 实现。"""

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
        return self._connection_impl   # 鸭子类型 DccConnection

    def as_script_engine(self):
        return self._script_engine     # 鸭子类型 DccScriptEngine

    def as_scene_info(self):
        return self._scene_info        # 鸭子类型 DccSceneInfo

    def as_snapshot(self):
        return self._snapshot          # 鸭子类型 DccSnapshot

    def as_scene_manager(self):
        return self._scene_manager     # 鸭子类型 DccSceneManager

    def as_transform(self):
        return self._transform         # 鸭子类型 DccTransform

    def as_hierarchy(self):
        return self._hierarchy         # 鸭子类型 DccHierarchy
```

::: tip 坐标约定
所有 `DccTransform` 实现必须使用**右手 Y-up 世界空间**，欧拉 XYZ 旋转以**度**为单位，平移以**厘米**为单位。适配器负责从各自的原生坐标系转换（如 Blender Z-up 弧度制、Unreal 厘米 Z-up）。
:::

::: info 可选子 Traits
四个跨 DCC 协议 trait（`DccSceneManager`、`DccTransform`、`DccRenderCapture`、`DccHierarchy`）均为**可选** — 如果 DCC 不支持，则从访问器返回 `None`，并在 `DccCapabilities` 中将对应字段设为 `False`，以便客户端在调用前检查支持情况。
:::
