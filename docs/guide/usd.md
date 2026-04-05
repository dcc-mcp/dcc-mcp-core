# USD Guide

USD (Universal Scene Description) support for DCC-MCP-Core.

## Overview

Provides:

- **Core USD types**: `SdfPath`, `VtValue`, `UsdAttribute`, `UsdPrim`, `UsdLayer`, `UsdStage`
- **USDA serialization**: Export stages as human-readable `.usda` text
- **JSON transport**: Serialize/deserialize stages for MCP IPC
- **DCC bridge**: Convert between `dcc-mcp-protocols` `SceneInfo` and `UsdStage`
- **Pure Rust**: No dependency on OpenUSD C++ library

::: warning
This crate does not link against the OpenUSD C++ library. It provides a compatible data model and serialization format for lightweight scene description exchange in the DCC-MCP ecosystem.
:::

## Quick Start

### Creating a Stage

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# Create a new stage
stage = UsdStage.new("my_scene")

# Define primitives
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")

# Set attributes
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
```

### Exporting

```python
# Export to USDA text
usda = stage.export_usda()
print(usda)

# Export to JSON (for IPC)
json_str = stage.to_json()
```

## SdfPath

USD scene graph path.

### Creating Paths

```python
from dcc_mcp_core import SdfPath

# Valid path
path = SdfPath.new("/World/Cube")
print(path)  # "/World/Cube"

# Check validity
print(path.is_valid())  # True

# Path components
print(path.prim_path)    # "/World"
print(path.name)        # "Cube"
print(path.parent_path)  # "/World"
```

### Path Operations

```python
# Append child
child = path.append_child("Material")
print(child)  # "/World/Cube/Material"

# Absolute path
abs_path = path.make_absolute()
```

### Path Validation Rules

- Must start with `/`
- Cannot contain empty components
- Cannot end with `/` (except for root `/`)
- Special characters must be escaped

## VtValue

USD value container.

### Supported Types

| Python Type | VtValue Constructor | USD Type |
|-------------|---------------------|----------|
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

### Creating Values

```python
# Scalars
v_int = VtValue.int(42)
v_float = VtValue.float(3.14)
v_bool = VtValue.bool(True)
v_string = VtValue.string("hello")

# Vectors
v_vec3 = VtValue.vec3f([1.0, 2.0, 3.0])

# Arrays
v_array = VtValue.int_array([1, 2, 3, 4, 5])
v_points = VtValue.vec3f_array([
    [0, 0, 0],
    [1, 0, 0],
    [0, 1, 0]
])
```

### Getting Values

```python
value = vt_value.get()
print(vt_value.type_name())  # e.g., "float3"
```

## UsdStage

Main stage container.

### Creating Stages

```python
# New stage
stage = UsdStage.new("my_scene")

# From USDA text
stage = UsdStage.parse_usda(usda_text)
```

### Defining Primitives

```python
# Define prims
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Sphere"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")
```

### Setting Attributes

```python
# Set various attribute types
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
stage.set_attribute("/World/Cube", "faceConnects", VtValue.int_array([0, 1, 2, 3, 4, 5]))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.double(24.0))
stage.set_attribute("/World", "name", VtValue.string("World"))
```

### Querying Stage

```python
# Check if prim exists
print(stage.has_prim("/World/Cube"))  # True

# Get prim
prim = stage.get_prim("/World/Cube")
print(prim.path)        # SdfPath
print(prim.type_name)   # "Mesh"
print(prim.attributes)  # List[UsdAttribute]

# Iterate all prims
for prim in stage.iter_prims():
    print(f"{prim.path} ({prim.type_name})")
```

## Serialization

### USDA Format

USDA is the human-readable text format for USD:

```python
# Export to USDA
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
        float3[] extent = [(1, 1, 1)]
    }
}
```

### JSON Format

JSON is compact and efficient for IPC:

```python
# Export to JSON
json_str = stage.to_json()

# Import from JSON
stage2 = UsdStage.from_json(json_str)
```

## DCC Bridge

Convert between `dcc-mcp-protocols` `SceneInfo` and `UsdStage`.

### SceneInfo to UsdStage

```python
from dcc_mcp_core import UsdStage
from dcc_mcp_protocols import SceneInfo

# From protocol scene info
scene_info = SceneInfo(
    name="my_scene",
    prim_count=100,
    active_layer="session"
)

usd_stage = UsdStage.from_scene_info(scene_info)
```

### UsdStage to SceneInfo

```python
# Convert USD stage to protocol scene info
scene_info = usd_stage.to_scene_info()
print(f"Name: {scene_info.name}")
print(f"Prims: {scene_info.prim_count}")
```

## Complete Example

### Creating a Simple Scene

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

# Create stage
stage = UsdStage.new("sample_scene")

# Define scene structure
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Geometries"), "Scope")
stage.define_prim(SdfPath.new("/World/Geometries/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Geometries/Sphere"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")

# Set cube attributes
stage.set_attribute("/World/Geometries/Cube", "extent", VtValue.vec3f([1, 1, 1]))
stage.set_attribute("/World/Geometries/Cube", "points", VtValue.vec3f_array([
    [0, 0, 0], [1, 0, 0], [1, 1, 0], [0, 1, 0]
]))

# Set sphere attributes
stage.set_attribute("/World/Geometries/Sphere", "extent", VtValue.vec3f([1, 1, 1]))

# Export
usda = stage.export_usda()
print(usda)
```

### Loading from File

```python
from dcc_mcp_core import UsdStage

# Read USDA file
with open("scene.usda", "r") as f:
    usda_text = f.read()

stage = UsdStage.parse_usda(usda_text)
print(f"Loaded: {stage.name}")
print(f"Prims: {len(list(stage.iter_prims()))}")
```

## Best Practices

### 1. Validate Paths

```python
from dcc_mcp_core import SdfPath

# Always validate before use
path = SdfPath.new("/World/Cube")
if path.is_valid():
    stage.define_prim(path, "Mesh")
```

### 2. Use Appropriate Value Types

```python
# Use float3 for positions
position = VtValue.vec3f([1.0, 2.0, 3.0])

# Use int arrays for connectivity
faces = VtValue.int_array([0, 1, 2, 3, 4, 5])
```

### 3. Batch Operations

```python
# Define multiple prims at once
prims = [
    (SdfPath.new("/World/Cube"), "Mesh"),
    (SdfPath.new("/World/Sphere"), "Mesh"),
    (SdfPath.new("/World/Cylinder"), "Mesh"),
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

- This is a pure Rust/USD-compatible data model, not a full OpenUSD implementation
- No C++ USD library dependency required
- When `usd-rs` stabilizes, this crate can be extended with direct C++ bridging
- Some advanced USD features may not be available
