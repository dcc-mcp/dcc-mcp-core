# USD 指南

DCC-MCP-Core 的 USD（通用场景描述）支持。

## 概述

提供：

- **核心 USD 类型**: `SdfPath`、`VtValue`、`UsdAttribute`、`UsdPrim`、`UsdLayer`、`UsdStage`
- **USDA 序列化**: 导出的场景为人类可读的 `.usda` 文本
- **JSON 传输**: 序列化和反序列化场景用于 MCP IPC
- **DCC 桥接**: 在 `dcc-mcp-protocols` `SceneInfo` 和 `UsdStage` 之间转换
- **纯 Rust**: 无需 OpenUSD C++ 库依赖

::: warning
此 crate 不链接 OpenUSD C++ 库。它为 DCC-MCP 生态系统中的轻量级场景描述交换提供兼容的数据模型和序列化格式。
:::

## 快速开始

### 创建 Stage

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# 创建新 stage
stage = UsdStage.new("my_scene")

# 定义图元
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")

# 设置属性
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
```

### 导出

```python
# 导出为 USDA 文本
usda = stage.export_usda()
print(usda)

# 导出为 JSON（用于 IPC）
json_str = stage.to_json()
```

## SdfPath

USD 场景图路径。

### 创建路径

```python
from dcc_mcp_core import SdfPath

# 有效路径
path = SdfPath.new("/World/Cube")
print(path)  # "/World/Cube"

# 检查有效性
print(path.is_valid())  # True

# 路径组件
print(path.prim_path)    # "/World"
print(path.name)        # "Cube"
print(path.parent_path)  # "/World"
```

### 路径操作

```python
# 追加子路径
child = path.append_child("Material")
print(child)  # "/World/Cube/Material"

# 绝对路径
abs_path = path.make_absolute()
```

## VtValue

USD 值容器。

### 支持的类型

| Python 类型 | VtValue 构造器 | USD 类型 |
|-------------|----------------|----------|
| `int` | `VtValue.int()` | `int` |
| `float` | `VtValue.float()` | `float` |
| `float` | `VtValue.double()` | `double` |
| `bool` | `VtValue.bool()` | `bool` |
| `str` | `VtValue.string()` | `string` |
| `[float, float, float]` | `VtValue.vec3f()` | `float3` |
| `[float, float, float]` | `VtValue.vec3d()` | `double3` |
| `[[float, float, float], ...]` | `VtValue.vec3f_array()` | `float3[]` |
| `[int, ...]` | `VtValue.int_array()` | `int[]` |
| `[str, ...]` | `VtValue.string_array()` | `string[]` |

### 创建值

```python
# 标量
v_int = VtValue.int(42)
v_float = VtValue.float(3.14)
v_bool = VtValue.bool(True)
v_string = VtValue.string("hello")

# 向量
v_vec3 = VtValue.vec3f([1.0, 2.0, 3.0])

# 数组
v_array = VtValue.int_array([1, 2, 3, 4, 5])
v_points = VtValue.vec3f_array([
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0]
])
```

## UsdStage

主要 stage 容器。

### 创建 Stage

```python
# 新 stage
stage = UsdStage.new("my_scene")

# 从 USDA 文本
stage = UsdStage.parse_usda(usda_text)
```

### 定义图元

```python
# 定义图元
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Sphere"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")
```

### 设置属性

```python
# 设置各种属性类型
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
stage.set_attribute("/World/Cube", "faceConnects", VtValue.int_array([0, 1, 2, 3, 4, 5]))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.double(24.0))
stage.set_attribute("/World", "name", VtValue.string("World"))
```

### 查询 Stage

```python
# 检查图元是否存在
print(stage.has_prim("/World/Cube"))  # True

# 获取图元
prim = stage.get_prim("/World/Cube")
print(prim.path)        # SdfPath
print(prim.type_name)   # "Mesh"
print(prim.attributes)  # List[UsdAttribute]

# 遍历所有图元
for prim in stage.iter_prims():
    print(f"{prim.path} ({prim.type_name})")
```

## 序列化

### USDA 格式

USDA 是人类可读的 USD 文本格式：

```python
# 导出为 USDA
usda = stage.export_usda()
print(usda)
```

### JSON 格式

JSON 紧凑高效，适合 IPC：

```python
# 导出为 JSON
json_str = stage.to_json()

# 从 JSON 导入
stage2 = UsdStage.from_json(json_str)
```

## DCC 桥接

在 `dcc-mcp-protocols` `SceneInfo` 和 `UsdStage` 之间转换。

### SceneInfo 到 UsdStage

```python
from dcc_mcp_core import UsdStage
from dcc_mcp_protocols import SceneInfo

# 从协议场景信息
scene_info = SceneInfo(
    name="my_scene",
    prim_count=100,
    active_layer="session"
)

usd_stage = UsdStage.from_scene_info(scene_info)
```

### UsdStage 到 SceneInfo

```python
# 转换 USD stage 为协议场景信息
scene_info = usd_stage.to_scene_info()
print(f"名称: {scene_info.name}")
print(f"图元数: {scene_info.prim_count}")
```

## 完整示例

### 创建简单场景

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# 创建 stage
stage = UsdStage.new("sample_scene")

# 定义场景结构
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Geometries"), "Scope")
stage.define_prim(SdfPath.new("/World/Geometries/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Geometries/Sphere"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")

# 设置立方体属性
stage.set_attribute("/World/Geometries/Cube", "extent", VtValue.vec3f([1, 1, 1]))
stage.set_attribute("/World/Geometries/Cube", "points", VtValue.vec3f_array([
    [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]
]))

# 设置球体属性
stage.set_attribute("/World/Geometries/Sphere", "extent", VtValue.vec3f([1, 1, 1]))

# 导出
usda = stage.export_usda()
print(usda)
```

## 限制

- 这是纯 Rust/USD 兼容数据模型，不是完整的 OpenUSD 实现
- 无需 C++ USD 库依赖
- `usd-rs` 稳定后，可以扩展此 crate 实现直接 C++ 桥接
- 某些高级 USD 功能可能不可用
