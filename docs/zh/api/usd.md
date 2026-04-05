# USD API

`dcc_mcp_core` (usd 模块)

DCC-MCP-Core 的 USD（通用场景描述）支持。

## 概述

提供：

- **核心 USD 类型**: `SdfPath`、`VtValue`、`UsdAttribute`、`UsdPrim`、`UsdLayer`、`UsdStage`
- **USDA 序列化**: 导出的场景为人类可读的 `.usda` 文本
- **JSON 传输**: 序列化和反序列化场景用于 MCP IPC
- **DCC 桥接**: 在 `dcc-mcp-protocols` `SceneInfo` 和 `UsdStage` 之间转换
- **PyO3 绑定**: `UsdStage`、`UsdPrim`、`SdfPath`、`VtValue` 暴露给 Python

::: warning
此 crate 不链接 OpenUSD C++ 库。它为轻量级场景描述交换提供兼容的数据模型和序列化格式。
:::

## UsdStage

USD 场景数据的主要 stage 容器。

### 创建 Stage

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

stage = UsdStage.new("my_scene")
```

### 定义图元

```python
# 在路径处定义图元
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")
```

### 设置属性

```python
# 设置各种属性类型
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
stage.set_attribute("/World/Cube", "faceConnects", VtValue.int_array([0, 1, 2]))
stage.set_attribute("/World/Cube", "points", VtValue.vec3f_array([[0, 0, 0], [1, 0, 0], [0, 1, 0]]))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.double(24.0))
```

### 查询图元

```python
# 检查图元是否存在
has_cube = stage.has_prim("/World/Cube")

# 获取图元
prim = stage.get_prim("/World/Cube")
print(prim.path)        # SdfPath
print(prim.type_name)   # "Mesh"
print(prim.attributes)  # 属性列表

# 列出所有图元
for prim in stage.iter_prims():
    print(prim.path)
```

## SdfPath

USD 场景图路径。

### 创建路径

```python
from dcc_mcp_core import SdfPath

# 创建有效路径
path = SdfPath.new("/World/Cube")
print(path)  # "/World/Cube"

# 检查有效性
print(path.is_valid())  # True

# 获取路径组件
print(path.prim_path)    # "/World"
print(path.name)        # "Cube"
print(path.parent_path)  # "/World"
```

### 路径操作

```python
# 追加子路径
child = path.append_child("Material")
print(child)  # "/World/Cube/Material"

# 获取绝对路径
abs_path = path.make_absolute()
```

## VtValue

USD 值容器，支持类型。

### 值类型

| 变体 | Python 类型 | USD 类型 |
|------|-------------|----------|
| `VtValue.int` | `int` | `int` |
| `VtValue.float` | `float` | `float` |
| `VtValue.double` | `float` | `double` |
| `VtValue.bool` | `bool` | `bool` |
| `VtValue.string` | `str` | `string` |
| `VtValue.vec3f` | `List[float, float, float]` | `float3` |
| `VtValue.vec3d` | `List[float, float, float]` | `double3` |
| `VtValue.vec3f_array` | `List[List[float]]` | `float3[]` |
| `VtValue.int_array` | `List[int]` | `int[]` |
| `VtValue.string_array` | `List[str]` | `string[]` |

### 创建值

```python
# 标量值
v_int = VtValue.int(42)
v_float = VtValue.float(3.14)
v_string = VtValue.string("hello")
v_vec3 = VtValue.vec3f([1.0, 2.0, 3.0])

# 数组值
v_array = VtValue.int_array([1, 2, 3, 4, 5])
v_points = VtValue.vec3f_array([[0, 0, 0], [1, 0, 0], [0, 1, 0]])
```

### 获取值

```python
# 获取 Python 值
value = vt_value.get()
print(value)

# 检查类型
print(vt_value.type_name())  # 例如 "float3"
```

## UsdAttribute

图元属性。

### 访问属性

```python
# 获取属性
attr = stage.get_attribute("/World/Cube", "extent")
if attr:
    print(attr.name)       # "extent"
    print(attr.value)      # VtValue
    print(attr.type_name) # "float3"
```

## 序列化

### 导出到 USDA

```python
# 导出为 USDA 文本
usda_text = stage.export_usda()
print(usda_text)
```

### 导出到 JSON

```python
# 导出为 JSON 用于 IPC 传输
json_str = stage.to_json()

# 从 JSON 导入
stage2 = UsdStage.from_json(json_str)
```

### 从 USDA 加载

```python
# 从 USDA 文本加载 stage
stage = UsdStage.parse_usda(usda_text)
```

## DCC 桥接

在 `SceneInfo` 和 `UsdStage` 之间转换。

```python
from dcc_mcp_core import UsdStage
from dcc_mcp_protocols import SceneInfo

# SceneInfo -> UsdStage
scene_info = SceneInfo(...)
usd_stage = UsdStage.from_scene_info(scene_info)

# UsdStage -> SceneInfo
scene_info = usd_stage.to_scene_info()
```

## 错误处理

```python
from dcc_mcp_core import UsdError

try:
    path = SdfPath.new("invalid/path")  # 必须以 / 开头
except UsdError as e:
    print(f"USD 错误: {e}")
```

## 性能说明

- USDA 导出是人类可读的但较慢
- JSON 序列化紧凑快速，适合 IPC
- 路径验证在构造时进行，不是惰性的
- 大数组（points、faceConnects）以 Arrow 格式高效存储
