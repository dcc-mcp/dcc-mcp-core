# USD API

`dcc_mcp_core` (usd module)

USD (Universal Scene Description) support for DCC-MCP-Core.

## Overview

Provides:

- **Core USD types**: `SdfPath`, `VtValue`, `UsdPrim`, `UsdStage`
- **USDA serialization**: Export stages as human-readable `.usda` text
- **JSON transport**: Serialize/deserialize stages for MCP IPC
- **DCC bridge**: Convert between `SceneInfo` JSON and `UsdStage`
- **Pure Rust**: No dependency on OpenUSD C++ library

::: warning
This crate provides a compatible data model and serialization format for lightweight scene description exchange. It does not link against the OpenUSD C++ library.
:::

## SdfPath

USD scene graph path (e.g. `/World/Cube`).

### Constructor

```python
from dcc_mcp_core import SdfPath

path = SdfPath("/World/Cube")
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `child(name)` | `SdfPath` | Append child segment |
| `parent()` | `SdfPath \| None` | Parent path |
| `__eq__(other)` | `bool` | Equality comparison |
| `__hash__()` | `int` | Hash value (usable as dict key) |
| `__str__()` | `str` | String representation |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_absolute` | `bool` | True if path starts with `/` |
| `name` | `str` | Last path element |

### Example

```python
path = SdfPath("/World/Cube")
print(path.is_absolute)  # True
print(path.name)         # Cube
child = path.child("Material")  # SdfPath("/World/Cube/Material")
parent = path.parent()   # SdfPath("/World")
```

## VtValue

USD variant value container.

### Factory Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `from_bool(v)` | `VtValue` | Create bool value |
| `from_int(v)` | `VtValue` | Create int value |
| `from_float(v)` | `VtValue` | Create float value |
| `from_string(v)` | `VtValue` | Create string value |
| `from_token(v)` | `VtValue` | Create token value |
| `from_asset(v)` | `VtValue` | Create asset value |
| `from_vec3f(x, y, z)` | `VtValue` | Create vec3 from floats |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `type_name` | `str` | USD type name (e.g. `float3`) |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `to_python()` | `bool \| int \| float \| str \| tuple \| list \| None` | Convert to Python primitive |

### Example

```python
v_bool = VtValue.from_bool(True)
v_int = VtValue.from_int(42)
v_float = VtValue.from_float(3.14)
v_vec3 = VtValue.from_vec3f(1.0, 2.0, 3.0)

print(v_vec3.to_python())   # (1.0, 2.0, 3.0)
print(v_vec3.type_name)    # float3
```

## UsdPrim

A prim (primitive) within a USD stage.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `path` | `SdfPath` | Prim path |
| `type_name` | `str` | Prim type (e.g. `Mesh`) |
| `active` | `bool` | Whether prim is active |
| `name` | `str` | Prim name |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `set_attribute(name, value)` | `None` | Set attribute value |
| `get_attribute(name)` | `VtValue \| None` | Get attribute value |
| `attribute_names()` | `list[str]` | List attribute names |
| `attributes_summary()` | `dict[str, str]` | Attribute names to type names |
| `has_api(schema)` | `bool` | Check if prim has API |

## UsdStage

Main stage container for USD scene data.

### Constructor

```python
from dcc_mcp_core import UsdStage

stage = UsdStage("my_scene")
```

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `name` | `str` | Stage name |
| `id` | `str` | Stage UUID |
| `default_prim` | `str \| None` | Default prim name |
| `up_axis` | `str` | Up axis (`X`, `Y`, or `Z`) |
| `meters_per_unit` | `float` | Meters per unit |
| `fps` | `float \| None` | Frames per second |
| `start_time_code` | `float \| None` | Start time code |
| `end_time_code` | `float \| None` | End time code |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `define_prim(path, type_name)` | `UsdPrim` | Define a prim |
| `get_prim(path)` | `UsdPrim \| None` | Get a prim |
| `has_prim(path)` | `bool` | Check if prim exists |
| `remove_prim(path)` | `bool` | Remove a prim |
| `traverse()` | `list[UsdPrim]` | Get all prims |
| `prims_of_type(type_name)` | `list[UsdPrim]` | Get prims of type |
| `set_attribute(prim_path, attr_name, value)` | `None` | Set attribute |
| `get_attribute(prim_path, attr_name)` | `VtValue \| None` | Get attribute |
| `metrics()` | `dict[str, int]` | Get stage metrics |
| `to_json()` | `str` | Export as JSON |
| `from_json(json)` *(staticmethod)* | `UsdStage` | Import from JSON |
| `export_usda()` | `str` | Export as USDA text |

### Example

```python
stage = UsdStage("my_scene")
stage.default_prim = "World"

stage.define_prim("/World", "Xform")
stage.define_prim("/World/Cube", "Mesh")
stage.set_attribute("/World/Cube", "extent", VtValue.from_vec3f(1, 1, 1))

# Serialize
json_str = stage.to_json()
usda = stage.export_usda()

# Query
for prim in stage.traverse():
    print(f"{prim.path} ({prim.type_name})")
```

## DCC Bridge Functions

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

## Performance Notes

- USDA export is human-readable but slower than JSON
- JSON serialization is compact and fast, ideal for IPC
- Path validation is performed on construction, not lazily
