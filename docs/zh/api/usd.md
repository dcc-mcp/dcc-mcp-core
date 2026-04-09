# USD API

`dcc_mcp_core` (usd 模块)

DCC-MCP-Core 的 USD（通用场景描述）支持。

## 概述

提供：

- **核心 USD 类型**: `SdfPath`、`VtValue`、`UsdPrim`、`UsdStage`
- **USDA 序列化**: 导出的场景为人类可读的 `.usda` 文本
- **JSON 传输**: 序列化和反序列化场景用于 MCP IPC
- **DCC 桥接**: 在 `SceneInfo` JSON 和 `UsdStage` 之间转换
- **纯 Rust**: 无需 OpenUSD C++ 库依赖

::: warning
此 crate 提供兼容的数据模型和序列化格式用于轻量级场景描述交换。它不链接 OpenUSD C++ 库。
:::

## SdfPath

USD 场景图路径（例如 `/World/Cube`）。

### 构造函数

```python
from dcc_mcp_core import SdfPath

path = SdfPath("/World/Cube")
```

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `child(name)` | `SdfPath` | 追加子路径段 |
| `parent()` | `SdfPath \| None` | 父路径 |

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `is_absolute` | `bool` | 路径是否以 `/` 开头 |
| `name` | `str` | 最后一个路径元素 |

### 示例

```python
path = SdfPath("/World/Cube")
print(path.is_absolute)  # True
print(path.name)         # Cube
child = path.child("Material")  # SdfPath("/World/Cube/Material")
parent = path.parent()   # SdfPath("/World")
```

## VtValue

USD 变体值容器。

### 工厂方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `from_bool(v)` | `VtValue` | 创建布尔值 |
| `from_int(v)` | `VtValue` | 创建整数值 |
| `from_float(v)` | `VtValue` | 创建浮点值 |
| `from_string(v)` | `VtValue` | 创建字符串值 |
| `from_token(v)` | `VtValue` | 创建 token 值 |
| `from_asset(v)` | `VtValue` | 创建 asset 值 |
| `from_vec3f(x, y, z)` | `VtValue` | 从浮点数创建 vec3 |

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `type_name` | `str` | USD 类型名称（例如 `float3`） |

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `to_python()` | `bool \| int \| float \| str \| tuple \| list \| None` | 转换为 Python 原生类型 |

### 示例

```python
v_bool = VtValue.from_bool(True)
v_int = VtValue.from_int(42)
v_float = VtValue.from_float(3.14)
v_vec3 = VtValue.from_vec3f(1.0, 2.0, 3.0)

print(v_vec3.to_python())   # (1.0, 2.0, 3.0)
print(v_vec3.type_name)    # float3
```

## UsdPrim

USD stage 中的图元。

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `path` | `SdfPath` | 图元路径 |
| `type_name` | `str` | 图元类型（例如 `Mesh`） |
| `active` | `bool` | 图元是否激活 |
| `name` | `str` | 图元名称 |

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `set_attribute(name, value)` | `None` | 设置属性值 |
| `get_attribute(name)` | `VtValue \| None` | 获取属性值 |
| `attribute_names()` | `list[str]` | 列出属性名称 |
| `attributes_summary()` | `dict[str, str]` | 属性名称到类型名称的映射 |
| `has_api(schema)` | `bool` | 检查是否有 API |

## UsdStage

USD 场景数据的主要容器。

### 构造函数

```python
from dcc_mcp_core import UsdStage

stage = UsdStage("my_scene")
```

### 属性

| 属性 | 类型 | 描述 |
|------|------|------|
| `name` | `str` | Stage 名称 |
| `id` | `str` | Stage UUID |
| `default_prim` | `str \| None` | 默认图元名称 |
| `up_axis` | `str` | 向上轴（`X`、`Y` 或 `Z`） |
| `meters_per_unit` | `float` | 每单位米数 |
| `fps` | `float \| None` | 每秒帧数 |
| `start_time_code` | `float \| None` | 开始时间码 |
| `end_time_code` | `float \| None` | 结束时间码 |

### 方法

| 方法 | 返回 | 描述 |
|------|------|------|
| `define_prim(path, type_name)` | `UsdPrim` | 定义图元 |
| `get_prim(path)` | `UsdPrim \| None` | 获取图元 |
| `has_prim(path)` | `bool` | 检查图元是否存在 |
| `remove_prim(path)` | `bool` | 删除图元 |
| `traverse()` | `list[UsdPrim]` | 获取所有图元 |
| `prims_of_type(type_name)` | `list[UsdPrim]` | 获取特定类型的图元 |
| `set_attribute(prim_path, attr_name, value)` | `None` | 设置属性 |
| `get_attribute(prim_path, attr_name)` | `VtValue \| None` | 获取属性 |
| `metrics()` | `dict[str, int]` | 获取 stage 指标 |
| `to_json()` | `str` | 导出为 JSON |
| `from_json(json)` | `UsdStage` | 从 JSON 导入 |
| `export_usda()` | `str` | 导出为 USDA 文本 |

### 示例

```python
stage = UsdStage("my_scene")
stage.default_prim = "World"

stage.define_prim("/World", "Xform")
stage.define_prim("/World/Cube", "Mesh")
stage.set_attribute("/World/Cube", "extent", VtValue.from_vec3f(1, 1, 1))

# 序列化
json_str = stage.to_json()
usda = stage.export_usda()

# 查询
for prim in stage.traverse():
    print(f"{prim.path} ({prim.type_name})")
```

## DCC 桥接函数

### scene_info_json_to_stage()

```python
from dcc_mcp_core import scene_info_json_to_stage

scene_info_json = '{"name": "my_scene", "prim_count": 100}'
usd_stage = scene_info_json_to_stage(scene_info_json, "maya")
```

### stage_to_scene_info_json()

```python
from dcc_mcp_core import stage_to_scene_info_json

scene_info_json = stage_to_scene_info_json(stage)
```

### units_to_mpu()

```python
from dcc_mcp_core import units_to_mpu

print(units_to_mpu("cm"))   # 0.01
print(units_to_mpu("m"))    # 1.0
```

### mpu_to_units()

```python
from dcc_mcp_core import mpu_to_units

print(mpu_to_units(0.01))   # cm
print(mpu_to_units(1.0))    # m
```

## 性能说明

- USDA 导出是人类可读的但比 JSON 慢
- JSON 序列化是紧凑和快速的，适合 IPC
- 路径验证在构造时执行，而不是惰性地执行
