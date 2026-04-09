# USD Guide

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

### Creating Paths

```python
from dcc_mcp_core import SdfPath

# Create from string
path = SdfPath("/World")
print(path)  # /World

# Child path
child = path.child("Cube")
print(child)  # /World/Cube
print(child.name)  # Cube
print(child.is_absolute)  # True

# Parent path
parent = child.parent()
print(parent)  # /World
```

### Path Properties

```python
path = SdfPath("/World/Cube")

print(path.is_absolute)  # True
print(path.name)         # Cube
print(path.parent())     # SdfPath("/World")
```

## VtValue

USD variant value container (bool, int, float, string, vec3f, etc.).

### Factory Methods

```python
from dcc_mcp_core import VtValue

# Scalars
v_bool   = VtValue.from_bool(True)
v_int    = VtValue.from_int(42)
v_float  = VtValue.from_float(3.14)
v_string = VtValue.from_string("hello")
v_token  = VtValue.from_token("normal")
v_asset  = VtValue.from_asset("/path/to/texture.png")

# Vectors (x, y, z as separate floats)
v_vec3 = VtValue.from_vec3f(1.0, 2.0, 3.0)
```

### Getting Values

```python
# Convert back to Python primitive
print(v_bool.to_python())    # True
print(v_int.to_python())     # 42
print(v_float.to_python())   # 3.14
print(v_vec3.to_python())    # (1.0, 2.0, 3.0)

# Type name
print(v_vec3.type_name)  # float3
```

## UsdStage

Main stage container for USD scene description.

### Creating Stages

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# New stage with name
stage = UsdStage("my_scene")
print(stage.name)  # my_scene
print(stage.id)    # UUID
```

### Defining Primitives

```python
# Define prims with path string
stage.define_prim("/World", "Xform")
stage.define_prim("/World/Cube", "Mesh")
stage.define_prim("/World/Sphere", "Mesh")
stage.define_prim("/World/Lights", "Scope")
```

### Setting Attributes

```python
# Set various attribute types
stage.set_attribute("/World/Cube", "extent", VtValue.from_vec3f(1, 1, 1))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.from_float(24.0))
stage.set_attribute("/World", "name", VtValue.from_string("World"))
```

### Querying Stage

```python
# Check if prim exists
print(stage.has_prim("/World/Cube"))  # True

# Get prim
prim = stage.get_prim("/World/Cube")
if prim:
    print(f"Path: {prim.path}")
    print(f"Type: {prim.type_name}")
    print(f"Active: {prim.active}")
    print(f"Name: {prim.name}")

# Get attribute
val = stage.get_attribute("/World/Cube", "extent")
if val:
    print(val.to_python())
```

### Traverse Primitives

```python
# Traverse all prims
for prim in stage.traverse():
    print(f"{prim.path} ({prim.type_name})")

# Get prims of specific type
for mesh in stage.prims_of_type("Mesh"):
    print(f"Mesh: {mesh.path}")
```

### Stage Properties

```python
# Default prim
stage.default_prim = "World"
print(stage.default_prim)  # World

# Up axis
stage.up_axis = "Y"
print(stage.up_axis)  # Y

# Units
stage.meters_per_unit = 0.01  # cm
print(stage.meters_per_unit)  # 0.01

# FPS
stage.fps = 24.0
print(stage.fps)  # 24.0

# Time range
stage.start_time_code = 1001.0
stage.end_time_code = 1050.0
```

### Remove Primitives

```python
stage.remove_prim("/World/Sphere")  # True if removed
print(stage.has_prim("/World/Sphere"))  # False
```

### Metrics

```python
metrics = stage.metrics()
print(f"Prim count: {metrics.get('prim_count', 0)}")
```

## Serialization

### Export to USDA

USDA is the human-readable text format:

```python
usda = stage.export_usda()
print(usda)
```

Output:
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

### JSON Format

JSON is compact for IPC:

```python
# Export to JSON
json_str = stage.to_json()

# Import from JSON
stage2 = UsdStage.from_json(json_str)
```

## DCC Bridge

Convert between `dcc-mcp-protocols` `SceneInfo` JSON and `UsdStage`.

### SceneInfo to UsdStage

```python
from dcc_mcp_core import scene_info_json_to_stage

# From protocol scene info JSON
scene_info_json = '{"name": "my_scene", "prim_count": 100}'
usd_stage = scene_info_json_to_stage(scene_info_json, "maya")
```

### UsdStage to SceneInfo

```python
from dcc_mcp_core import stage_to_scene_info_json

# Convert USD stage to protocol scene info JSON
scene_info_json = stage_to_scene_info_json(stage)
print(scene_info_json)  # {"name": "my_scene", ...}
```

### Unit Conversion

```python
from dcc_mcp_core import units_to_mpu, mpu_to_units

# Convert unit string to metersPerUnit
print(units_to_mpu("cm"))   # 0.01
print(units_to_mpu("m"))    # 1.0
print(units_to_mpu("inch")) # 0.0254

# Convert metersPerUnit to unit string
print(mpu_to_units(0.01))   # cm
print(mpu_to_units(1.0))    # m
```

## Complete Example

### Creating a Simple Scene

```python
from dcc_mcp_core import UsdStage, VtValue

# Create stage
stage = UsdStage("sample_scene")
stage.default_prim = "World"

# Define scene structure
stage.define_prim("/World", "Xform")
stage.define_prim("/World/Geometries", "Scope")
stage.define_prim("/World/Geometries/Cube", "Mesh")
stage.define_prim("/World/Geometries/Sphere", "Mesh")
stage.define_prim("/World/Lights", "Scope")

# Set cube attributes
stage.set_attribute("/World/Geometries/Cube", "extent", VtValue.from_vec3f(1, 1, 1))

# Export
usda = stage.export_usda()
print(usda)
```

### Loading from JSON

```python
# Export
json_str = stage.to_json()

# Load back
restored = UsdStage.from_json(json_str)
print(f"Restored: {restored.name}")
print(f"Prims: {len(restored.traverse())}")
```

## Best Practices

### 1. Validate Paths

```python
# Always check prim exists
if stage.has_prim("/World/Cube"):
    prim = stage.get_prim("/World/Cube")
    print(prim.type_name)
```

### 2. Use Appropriate Value Types

```python
# Use from_vec3f for positions (takes x, y, z separately)
position = VtValue.from_vec3f(1.0, 2.0, 3.0)

# Use from_int for counts
count = VtValue.from_int(42)

# Use from_string for names
name = VtValue.from_string("Sphere")
```

### 3. Batch Operations

```python
# Define multiple prims
prims = [
    ("/World/Cube", "Mesh"),
    ("/World/Sphere", "Mesh"),
    ("/World/Cylinder", "Mesh"),
]

for path, type_name in prims:
    stage.define_prim(path, type_name)
```

### 4. Use JSON for IPC

```python
# For network transmission, use JSON
json_str = stage.to_json()
send_over_ipc(json_str)

# For human-readable output, use USDA
usda = stage.export_usda()
save_to_file(usda)
```

## Limitations

- Pure Rust/USD-compatible data model, not a full OpenUSD implementation
- No C++ USD library dependency required
- Some advanced USD features may not be available
