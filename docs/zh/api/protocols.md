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

## ToolDeclaration

Skill 提供的单个 MCP 工具的轻量声明。从 SKILL.md frontmatter 的 `tools:` 数组中解析而来——
无需执行脚本即可完成发现。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `name` | `str` | — | 工具名称（MCP 工具标识符） |
| `description` | `str` | `""` | 人类可读描述 |
| `input_schema` | `str` | `""` | 输入参数的 JSON Schema 字符串 |
| `output_schema` | `str` | `""` | 输出的 JSON Schema 字符串（空 = 任意类型） |
| `read_only` | `bool` | `False` | True = 不修改 DCC 状态 |
| `destructive` | `bool` | `False` | True = 会删除或覆盖数据 |
| `idempotent` | `bool` | `False` | True = 可安全多次调用 |
| `source_file` | `str` | `""` | 相对于 Skill `scripts/` 目录的脚本路径 |

```python
from dcc_mcp_core import ToolDeclaration

td = ToolDeclaration(
    name="create_sphere",
    description="创建多边形球体",
    input_schema='{"type": "object", "required": ["radius"], "properties": {"radius": {"type": "number"}}}',
    output_schema=None,
    read_only=False,
    destructive=False,
    idempotent=False,
    source_file="scripts/create_sphere.py",
)

# 所有字段均可读写
td.name, td.description, td.input_schema, td.output_schema
td.read_only, td.destructive, td.idempotent, td.source_file
```

**与 SkillMetadata 的关系**：`skill.tools` 为 `List[ToolDeclaration]`，由 SKILL.md `tools:` 字段填充。
每个条目对应一个 MCP 工具。若未声明 `tools:`，Skill 回退为自动扫描 `scripts/` 目录下的脚本文件。



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

---

## DCC Adapter Traits

DCC 集成包实现这些 trait，将其应用连接到 MCP 生态系统。所有方法均为同步调用。

### 架构概览

```
DccAdapter              — 顶层 trait
  ├── DccConnection         — 连接生命周期
  ├── DccScriptEngine       — 脚本执行（Python / MEL / MaxScript / …）
  ├── DccSceneInfo          — 场景信息查询
  └── DccSnapshot           — 视口截图

跨 DCC 协议 Traits（通用，可选实现）
  ├── DccSceneManager       — 场景/文件管理、选择、可见性
  ├── DccTransform          — 对象 TRS 变换与包围盒
  ├── DccRenderCapture      — 视口截图与场景渲染输出
  └── DccHierarchy          — 父子层级与分组操作
```

### DccAdapter

顶层 trait，在 DCC 集成包中实现。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `info()` | `DccInfo` | 静态应用信息（类型、版本、PID、平台） |
| `capabilities()` | `DccCapabilities` | 功能标志 — 声明哪些子 trait 可用 |
| `as_connection()` | `DccConnection \| None` | 连接生命周期接口 |
| `as_script_engine()` | `DccScriptEngine \| None` | 脚本执行接口 |
| `as_scene_info()` | `DccSceneInfo \| None` | 场景信息查询接口 |
| `as_snapshot()` | `DccSnapshot \| None` | 截图/捕获接口 |
| `as_scene_manager()` | `DccSceneManager \| None` | 通用场景管理（可选） |
| `as_transform()` | `DccTransform \| None` | 通用对象 TRS（可选） |
| `as_render_capture()` | `DccRenderCapture \| None` | 渲染/捕获接口（可选） |
| `as_hierarchy()` | `DccHierarchy \| None` | 场景层级接口（可选） |

### DccConnection

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `connect()` | `None` | 建立到 DCC 的连接 |
| `disconnect()` | `None` | 断开连接 |
| `is_connected()` | `bool` | 连接是否存活 |
| `health_check()` | `int` | 往返 ping（毫秒） |

### DccScriptEngine

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `execute_script(code, language, timeout_ms)` | `ScriptResult` | 在 DCC 内执行脚本 |
| `supported_languages()` | `list[ScriptLanguage]` | 该 DCC 支持的脚本语言 |

### DccSceneInfo

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_scene_info()` | `SceneInfo` | 当前场景信息 |
| `list_objects()` | `list[tuple[str, str]]` | 所有场景对象的 `(名称, 类型)` 列表 |
| `get_selection()` | `list[str]` | 当前选中对象名称 |

### DccSnapshot

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `capture_viewport(viewport, width, height, format)` | `CaptureResult` | 将视口截图为 PNG / JPEG / WebP |

### DccSceneManager

通用场景与文件管理。支持 Maya、Blender、3dsMax、Unreal、Unity、Photoshop、Figma。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_scene_info()` | `SceneInfo` | 当前场景/文档元数据 |
| `list_objects(object_type)` | `list[SceneObject]` | 所有对象；传 `None` 返回全部 |
| `new_scene(save_prompt)` | `SceneInfo` | 创建新空白场景 |
| `open_file(file_path, force)` | `SceneInfo` | 从磁盘打开场景 |
| `save_file(file_path)` | `str` | 保存场景；`None` = 原位保存 |
| `export_file(file_path, format, selection_only)` | `str` | 导出为 FBX / OBJ / USD / PNG 等 |
| `get_selection()` | `list[str]` | 当前选中对象名称 |
| `set_selection(object_names)` | `list[str]` | 替换选择集 |
| `select_by_type(object_type)` | `list[str]` | 按类型全选 |
| `set_visibility(object_name, visible)` | `bool` | 切换对象/图层可见性 |

### DccTransform

通用 TRS 接口。坐标约定：右手 Y-up 世界空间，欧拉 XYZ（度），厘米单位。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_transform(object_name)` | `ObjectTransform` | 世界空间 TRS |
| `set_transform(object_name, translate, rotate, scale)` | `ObjectTransform` | 更新 TRS；传 `None` 保持不变 |
| `get_bounding_box(object_name)` | `BoundingBox` | 世界空间 AABB |
| `rename_object(old_name, new_name)` | `str` | 重命名对象；返回新长名称 |

### DccRenderCapture

视口截图与场景渲染输出。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `capture_viewport(viewport, width, height, format)` | `CaptureResult` | 活动/指定视口截图 |
| `render_scene(output_path, width, height, renderer)` | `RenderOutput` | 完整渲染输出到磁盘 |
| `get_render_settings()` | `dict[str, str]` | 当前渲染设置 |
| `set_render_settings(settings)` | `None` | 更新渲染设置 |

### DccHierarchy

父子对象图谱 — Maya DAG、Blender 集合、UE 关卡层级、Unity 场景图、Photoshop 图层组、Figma 框架。

| 方法 | 返回值 | 说明 |
|------|--------|------|
| `get_hierarchy()` | `list[SceneNode]` | 完整场景树（根节点含嵌套子节点） |
| `get_children(object_name)` | `list[SceneObject]` | 直接子节点；`None` = 场景根节点 |
| `get_parent(object_name)` | `str \| None` | 父节点名称；`None` = 位于场景根 |
| `group_objects(object_names, group_name, parent)` | `SceneObject` | 在新容器下分组 |
| `ungroup(group_name)` | `list[str]` | 解散分组；子对象移至分组父级 |
| `reparent(object_name, new_parent, preserve_world_transform)` | `SceneObject` | 更改父级 |

---

## DCC 数据类型

### ScriptLanguage

DCC 应用程序支持的脚本语言枚举。

| 值 | 说明 |
|----|------|
| `PYTHON` | Python |
| `MEL` | Maya Embedded Language |
| `MAXSCRIPT` | 3ds Max MAXScript |
| `HSCRIPT` | Houdini HScript |
| `VEX` | Houdini VEX |
| `LUA` | Lua |
| `CSHARP` | C#（Unity、Unreal） |
| `BLUEPRINT` | Unreal Engine Blueprint |

```python
from dcc_mcp_core import ScriptLanguage

lang = ScriptLanguage.PYTHON
assert lang == ScriptLanguage.PYTHON
```

### DccErrorCode

DCC 适配器操作的错误码枚举。

| 值 | 说明 |
|----|------|
| `CONNECTION_FAILED` | 无法连接到 DCC 进程 |
| `TIMEOUT` | 操作超时 |
| `SCRIPT_ERROR` | 脚本执行失败 |
| `NOT_RESPONDING` | DCC 无响应 |
| `UNSUPPORTED` | 此 DCC 不支持该操作 |
| `PERMISSION_DENIED` | 权限不足 |
| `INVALID_INPUT` | 输入参数无效 |
| `SCENE_ERROR` | 场景操作失败 |
| `INTERNAL` | DCC 适配器内部错误 |

```python
from dcc_mcp_core import DccErrorCode, DccError

err = DccError(DccErrorCode.TIMEOUT, "操作超时", recoverable=True)
assert err.code == DccErrorCode.TIMEOUT
```

### DccInfo

运行中的 DCC 应用程序实例的静态信息。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `dcc_type` | `str` | — | 应用类型（`"maya"`、`"blender"` 等） |
| `version` | `str` | — | 应用版本字符串 |
| `platform` | `str` | — | 操作系统平台（`"windows"`、`"linux"`、`"macos"`） |
| `pid` | `int` | — | 进程 ID |
| `python_version` | `str \| None` | `None` | 内嵌 Python 版本（如有） |
| `metadata` | `dict[str, str]` | `{}` | 任意扩展键值信息 |

```python
from dcc_mcp_core import DccInfo

info = DccInfo(
    dcc_type="maya",
    version="2024.2",
    platform="windows",
    pid=1234,
    python_version="3.10.11",
)
d = info.to_dict()  # 序列化为普通字典
```

### DccCapabilities

声明 DCC 适配器支持哪些子 trait 的功能标志。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `script_languages` | `list[ScriptLanguage]` | `[]` | 支持的脚本语言 |
| `scene_info` | `bool` | `False` | 是否实现 `DccSceneInfo` |
| `snapshot` | `bool` | `False` | 是否实现 `DccSnapshot` |
| `undo_redo` | `bool` | `False` | 支持撤销/重做 |
| `progress_reporting` | `bool` | `False` | 支持进度回调 |
| `file_operations` | `bool` | `False` | 支持文件打开/保存/导出 |
| `selection` | `bool` | `False` | 支持对象选择 |
| `scene_manager` | `bool` | `False` | 是否实现 `DccSceneManager` |
| `transform` | `bool` | `False` | 是否实现 `DccTransform` |
| `render_capture` | `bool` | `False` | 是否实现 `DccRenderCapture` |
| `hierarchy` | `bool` | `False` | 是否实现 `DccHierarchy` |
| `extensions` | `dict[str, bool]` | `{}` | 任意扩展功能标志 |

```python
from dcc_mcp_core import DccCapabilities, ScriptLanguage

caps = DccCapabilities(
    script_languages=[ScriptLanguage.PYTHON, ScriptLanguage.MEL],
    scene_info=True,
    snapshot=True,
    file_operations=True,
)
if caps.scene_manager:
    print("场景管理器可用")
```

### DccError

DCC 适配器操作失败时抛出或返回的错误类型。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `code` | `DccErrorCode` | — | 错误类别 |
| `message` | `str` | — | 人类可读描述 |
| `details` | `str \| None` | `None` | 可选扩展详情 |
| `recoverable` | `bool` | `False` | 重试是否可能成功 |

```python
from dcc_mcp_core import DccError, DccErrorCode

err = DccError(DccErrorCode.SCRIPT_ERROR, "NameError: name 'foo' is not defined")
print(err.code, err.recoverable)
```

### ScriptResult

`DccScriptEngine.execute_script()` 的返回结果。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `success` | `bool` | — | 执行是否成功 |
| `execution_time_ms` | `int` | — | 执行耗时（毫秒） |
| `output` | `str \| None` | `None` | 捕获的 stdout / 返回值 |
| `error` | `str \| None` | `None` | 失败时的错误信息 |
| `context` | `dict[str, str]` | `{}` | 执行上下文键值对 |

```python
from dcc_mcp_core import ScriptResult

r = ScriptResult(success=True, execution_time_ms=12, output="42")
d = r.to_dict()
```

### SceneStatistics

轻量级场景统计摘要。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `object_count` | `int` | `0` | 场景对象总数 |
| `vertex_count` | `int` | `0` | 网格顶点总数 |
| `polygon_count` | `int` | `0` | 多边形总数 |
| `material_count` | `int` | `0` | 唯一材质数 |
| `texture_count` | `int` | `0` | 唯一纹理数 |
| `light_count` | `int` | `0` | 光源数量 |
| `camera_count` | `int` | `0` | 摄像机数量 |

### SceneInfo

当前打开场景/文档的信息。

| 字段 | 类型 | 默认值 | 说明 |
|------|------|--------|------|
| `file_path` | `str` | `""` | 磁盘绝对路径；未保存时为空 |
| `name` | `str` | `"untitled"` | 场景/文档名称 |
| `modified` | `bool` | `False` | 是否有未保存更改 |
| `format` | `str` | `""` | 文件格式扩展名（`.ma`、`.blend` 等） |
| `frame_range` | `tuple[float, float] \| None` | `None` | `(起始帧, 结束帧)` |
| `current_frame` | `float \| None` | `None` | 当前活动帧 |
| `fps` | `float \| None` | `None` | 帧率 |
| `up_axis` | `str \| None` | `None` | `"Y"` 或 `"Z"` |
| `units` | `str \| None` | `None` | 线性单位（`"cm"`、`"m"` 等） |
| `statistics` | `SceneStatistics` | 默认值 | 场景对象计数 |
| `metadata` | `dict[str, str]` | `{}` | 任意扩展数据 |

---

## 跨 DCC 数据模型

### ObjectTransform

| 字段 | 类型 | 说明 |
|------|------|------|
| `translate` | `[float, float, float]` | 世界空间 XYZ（厘米） |
| `rotate` | `[float, float, float]` | 欧拉 XYZ 角（度） |
| `scale` | `[float, float, float]` | 非均匀缩放 (sx, sy, sz) |

```python
from dcc_mcp_core import ObjectTransform

t = ObjectTransform(
    translate=[10.0, 0.0, 5.0],
    rotate=[0.0, 45.0, 0.0],
    scale=[1.0, 1.0, 1.0],
)
identity = ObjectTransform.identity()  # 原点，无旋转，缩放=1
```

### BoundingBox

| 字段 | 类型 | 说明 |
|------|------|------|
| `min` | `[float, float, float]` | 世界空间最小角（cm） |
| `max` | `[float, float, float]` | 世界空间最大角（cm） |

```python
from dcc_mcp_core import BoundingBox

bb = BoundingBox(min=[-1.0, 0.0, -1.0], max=[1.0, 2.0, 1.0])
bb.center()  # [0.0, 1.0, 0.0]
bb.size()    # [2.0, 2.0, 2.0]
```

### SceneObject

场景中任意对象、图层或 Actor 的轻量描述。

| 字段 | 类型 | 说明 |
|------|------|------|
| `name` | `str` | 短叶节点名（如 `pCube1`） |
| `long_name` | `str` | 完整路径/唯一 ID（如 `\|group1\|pCube1`） |
| `object_type` | `str` | 类型字符串（`mesh`、`light`、`camera` 等） |
| `parent` | `str \| None` | 父节点长名称；`None` = 场景根 |
| `visible` | `bool` | 对象是否可见 |
| `metadata` | `dict[str, str]` | 任意扩展数据 |

### SceneNode

| 字段 | 类型 | 说明 |
|------|------|------|
| `object` | `SceneObject` | 该节点的对象 |
| `children` | `list[SceneNode]` | 直接子节点（递归嵌套） |

### FrameRange

| 字段 | 类型 | 说明 |
|------|------|------|
| `start` | `float` | 起始帧（含） |
| `end` | `float` | 结束帧（含） |
| `fps` | `float` | 帧率 |
| `current` | `float` | 当前活动帧 |

### RenderOutput

`DccRenderCapture.render_scene()` 的返回结果。

| 字段 | 类型 | 说明 |
|------|------|------|
| `file_path` | `str` | 渲染图像的绝对路径 |
| `width` | `int` | 图像宽度（像素） |
| `height` | `int` | 图像高度（像素） |
| `format` | `str` | 文件格式（`png`、`exr`、`jpg`） |
| `render_time_ms` | `int` | 渲染耗时（毫秒） |
