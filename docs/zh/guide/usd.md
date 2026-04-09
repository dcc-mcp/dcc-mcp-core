# USD 指南

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

### 创建路径

```python
from dcc_mcp_core import SdfPath

# 从字符串创建
path = SdfPath("/World")
print(path)  # /World

# 子路径
child = path.child("Cube")
print(child)  # /World/Cube
print(child.name)  # Cube
print(child.is_absolute)  # True

# 父路径
parent = child.parent()
print(parent)  # /World
```

### 路径属性

```python
path = SdfPath("/World/Cube")

print(path.is_absolute)  # True
print(path.name)         # Cube
print(path.parent())     # SdfPath("/World")
```

## VtValue

USD 变体值容器（bool、int、float、string、vec3f 等）。

### 工厂方法

```python
from dcc_mcp_core import VtValue

# 标量
v_bool   = VtValue.from_bool(True)
v_int    = VtValue.from_int(42)
v_float  = VtValue.from_float(3.14)
v_string = VtValue.from_string("hello")
v_token  = VtValue.from_token("normal")
v_asset  = VtValue.from_asset("/path/to/texture.png")

# 向量（x, y, z 作为单独的浮点数）
v_vec3 = VtValue.from_vec3f(1.0, 2.0, 3.0)
```

### 获取值

```python
# 转换回 Python 原生类型
print(v_bool.to_python())    # True
print(v_int.to_python())     # 42
print(v_float.to_python())   # 3.14
print(v_vec3.to_python())    # (1.0, 2.0, 3.0)

# 类型名称
print(v_vec3.type_name)  # float3
```

## UsdStage

USD 场景描述的主要容器。

### 创建 Stage

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# 用名称创建新 stage
stage = UsdStage("my_scene")
print(stage.name)  # my_scene
print(stage.id)    # UUID
```

### 定义图元

```python
# 用路径字符串定义图元
stage.define_prim("/World", "Xform")
stage.define_prim("/World/Cube", "Mesh")
stage.define_prim("/World/Sphere", "Mesh")
stage.define_prim("/World/Lights", "Scope")
```

### 设置属性

```python
# 设置各种属性类型
stage.set_attribute("/World/Cube", "extent", VtValue.from_vec3f(1, 1, 1))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.from_float(24.0))
stage.set_attribute("/World", "name", VtValue.from_string("World"))
```

### 查询 Stage

```python
# 检查图元是否存在
print(stage.has_prim("/World/Cube"))  # True

# 获取图元
prim = stage.get_prim("/World/Cube")
if prim:
    print(f"路径: {prim.path}")
    print(f"类型: {prim.type_name}")
    print(f"激活: {prim.active}")
    print(f"名称: {prim.name}")

# 获取属性
val = stage.get_attribute("/World/Cube", "extent")
if val:
    print(val.to_python())
```

### 遍历图元

```python
# 遍历所有图元
for prim in stage.traverse():
    print(f"{prim.path} ({prim.type_name})")

# 获取特定类型的图元
for mesh in stage.prims_of_type("Mesh"):
    print(f"网格: {mesh.path}")
```

### Stage 属性

```python
# 默认图元
stage.default_prim = "World"
print(stage.default_prim)  # World

# 向上轴
stage.up_axis = "Y"
print(stage.up_axis)  # Y

# 单位
stage.meters_per_unit = 0.01  # cm
print(stage.meters_per_unit)  # 0.01

# FPS
stage.fps = 24.0
print(stage.fps)  # 24.0

# 时间范围
stage.start_time_code = 1001.0
stage.end_time_code = 1050.0
```

### 删除图元

```python
stage.remove_prim("/World/Sphere")  # True 如果删除成功
print(stage.has_prim("/World/Sphere"))  # False
```

### 指标

```python
metrics = stage.metrics()
print(f"图元数量: {metrics.get('prim_count', 0)}")
```

## 序列化

### 导出为 USDA

USDA 是人类可读的文本格式：

```python
usda = stage.export_usda()
print(usda)
```

输出：
```usda
#usda 1.0
(
    defaultPrim = "World"
)

def Xform "World"
{
    def Mesh "Cube"
    {
        float3 extent = [(1, 1, 1)]
    }
}
```

### JSON 格式

JSON 紧凑适合 IPC：

```python
# 导出为 JSON
json_str = stage.to_json()

# 从 JSON 导入
stage2 = UsdStage.from_json(json_str)
```

## DCC 桥接

在 `dcc-mcp-protocols` `SceneInfo` JSON 和 `UsdStage` 之间转换。

### SceneInfo 到 UsdStage

```python
from dcc_mcp_core import scene_info_json_to_stage

# 从协议场景信息 JSON
scene_info_json = '{"name": "my_scene", "prim_count": 100}'
usd_stage = scene_info_json_to_stage(scene_info_json, "maya")
```

### UsdStage 到 SceneInfo

```python
from dcc_mcp_core import stage_to_scene_info_json

# 将 USD stage 转换为协议场景信息 JSON
scene_info_json = stage_to_scene_info_json(stage)
print(scene_info_json)  # {"name": "my_scene", ...}
```

### 单位转换

```python
from dcc_mcp_core import units_to_mpu, mpu_to_units

# 将单位字符串转换为 metersPerUnit
print(units_to_mpu("cm"))   # 0.01
print(units_to_mpu("m"))    # 1.0
print(units_to_mpu("inch")) # 0.0254

# 将 metersPerUnit 转换为单位字符串
print(mpu_to_units(0.01))   # cm
print(mpu_to_units(1.0))    # m
```

## 完整示例

### 创建简单场景

```python
from dcc_mcp_core import UsdStage, VtValue

# 创建 stage
stage = UsdStage("sample_scene")
stage.default_prim = "World"

# 定义场景结构
stage.define_prim("/World", "Xform")
stage.define_prim("/World/Geometries", "Scope")
stage.define_prim("/World/Geometries/Cube", "Mesh")
stage.define_prim("/World/Geometries/Sphere", "Mesh")
stage.define_prim("/World/Lights", "Scope")

# 设置立方体属性
stage.set_attribute("/World/Geometries/Cube", "extent", VtValue.from_vec3f(1, 1, 1))

# 导出
usda = stage.export_usda()
print(usda)
```

### 从 JSON 加载

```python
# 导出
json_str = stage.to_json()

# 恢复
restored = UsdStage.from_json(json_str)
print(f"已恢复: {restored.name}")
print(f"图元: {len(restored.traverse())}")
```

## 最佳实践

### 1. 验证路径

```python
# 总是检查图元是否存在
if stage.has_prim("/World/Cube"):
    prim = stage.get_prim("/World/Cube")
    print(prim.type_name)
```

### 2. 使用适当的值类型

```python
# 对位置使用 from_vec3f（分别传入 x, y, z）
position = VtValue.from_vec3f(1.0, 2.0, 3.0)

# 对计数使用 from_int
count = VtValue.from_int(42)

# 对名称使用 from_string
name = VtValue.from_string("Sphere")
```

### 3. 批量操作

```python
# 定义多个图元
prims = [
    ("/World/Cube", "Mesh"),
    ("/World/Sphere", "Mesh"),
    ("/World/Cylinder", "Mesh"),
]

for path, type_name in prims:
    stage.define_prim(path, type_name)
```

### 4. 使用 JSON 进行 IPC

```python
# 对于网络传输，使用 JSON
json_str = stage.to_json()
send_over_ipc(json_str)

# 对于人类可读输出，使用 USDA
usda = stage.export_usda()
save_to_file(usda)
```

## 限制

- 纯 Rust/USD 兼容数据模型，不是完整的 OpenUSD 实现
- 无需 C++ USD 库依赖
- 某些高级 USD 功能可能不可用
