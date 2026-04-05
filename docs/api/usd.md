# USD API

`dcc_mcp_core` (usd module)

USD (Universal Scene Description) support for DCC-MCP-Core.

## Overview

Provides:

- **Core USD types**: `SdfPath`, `VtValue`, `UsdAttribute`, `UsdPrim`, `UsdLayer`, `UsdStage`
- **USDA serialization**: Export stages as human-readable `.usda` text
- **JSON transport**: Serialize/deserialize stages for MCP IPC
- **DCC bridge**: Convert between `dcc-mcp-protocols` `SceneInfo` and `UsdStage`
- **Pure Rust**: No dependency on OpenUSD C++ library

::: warning
This crate does not link against the OpenUSD C++ library. It provides a compatible data model and serialization format for lightweight scene description exchange.
:::

## UsdStage

Main stage container for USD scene data.

### Creating a Stage

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue

stage = UsdStage.new("my_scene")
```

### Defining Primitives

```python
# Define a prim at a path
stage.define_prim(SdfPath.new("/World"), "Xform")
stage.define_prim(SdfPath.new("/World/Cube"), "Mesh")
stage.define_prim(SdfPath.new("/World/Lights"), "Scope")
```

### Setting Attributes

```python
# Set various attribute types
stage.set_attribute("/World/Cube", "extent", VtValue.vec3f([1.0, 1.0, 1.0]))
stage.set_attribute("/World/Cube", "faceConnects", VtValue.int_array([0, 1, 2]))
stage.set_attribute("/World/Cube", "points", VtValue.vec3f_array([[0, 0, 0], [1, 0, 0], [0, 1, 0]]))
stage.set_attribute("/World", "timeCodesPerSecond", VtValue.double(24.0))
```

### Querying Primitives

```python
# Check if a prim exists
has_cube = stage.has_prim("/World/Cube")

# Get a prim
prim = stage.get_prim("/World/Cube")
print(prim.path)        # SdfPath
print(prim.type_name)   # "Mesh"
print(prim.attributes)  # List of attributes

# List all prims
for prim in stage.iter_prims():
    print(prim.path)
```

## SdfPath

USD scene graph path.

### Creating Paths

```python
from dcc_mcp_core import SdfPath

# Create a valid path
path = SdfPath.new("/World/Cube")
print(path)  # "/World/Cube"

# Check validity
print(path.is_valid())  # True

# Get path components
print(path.prim_path)    # "/World"
print(path.name)        # "Cube"
print(path.parent_path)  # "/World"
```

### Path Operations

```python
# Append a child path
child = path.append_child("Material")
print(child)  # "/World/Cube/Material"

# Get absolute path
abs_path = path.make_absolute()
```

## VtValue

USD value container with type support.

### Value Types

| Variant | Python Type | USD Type |
|---------|-------------|----------|
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

### Creating Values

```python
# Scalar values
v_int = VtValue.int(42)
v_float = VtValue.float(3.14)
v_string = VtValue.string("hello")
v_vec3 = VtValue.vec3f([1.0, 2.0, 3.0])

# Array values
v_array = VtValue.int_array([1, 2, 3, 4, 5])
v_points = VtValue.vec3f_array([[0, 0, 0], [1, 0, 0], [0, 1, 0]])
```

### Getting Values

```python
# Get the Python value
value = vt_value.get()
print(value)

# Check the type
print(vt_value.type_name())  # e.g., "float3"
```

## UsdAttribute

Prim attribute.

### Accessing Attributes

```python
# Get an attribute
attr = stage.get_attribute("/World/Cube", "extent")
if attr:
    print(attr.name)       # "extent"
    print(attr.value)      # VtValue
    print(attr.type_name) # "float3"
```

### Attribute Operations

```python
# Set attribute value
stage.set_attribute("/World/Cube", "visibility", VtValue.string("inherited"))

# Get all attributes of a prim
attrs = prim.attributes
for attr in attrs:
    print(f"{attr.name}: {attr.type_name}")
```

## Serialization

### Export to USDA

```python
# Export as USDA text
usda_text = stage.export_usda()
print(usda_text)
```

### Export to JSON

```python
# Export as JSON for IPC transport
json_str = stage.to_json()

# Import from JSON
stage2 = UsdStage.from_json(json_str)
```

### Load from USDA

```python
# Load a stage from USDA text
stage = UsdStage.parse_usda(usda_text)
```

## DCC Bridge

Convert between `SceneInfo` and `UsdStage`.

```python
from dcc_mcp_core import UsdStage
from dcc_mcp_protocols import SceneInfo

# Convert SceneInfo -> UsdStage
scene_info = SceneInfo(...)
usd_stage = UsdStage.from_scene_info(scene_info)

# Convert UsdStage -> SceneInfo
scene_info = usd_stage.to_scene_info()
```

## DccSceneInfo

UsdStage implements the standard scene info trait.

```python
# UsdStage can be queried through the unified adapter interface
from dcc_mcp_protocols import DccAdapter

adapter = DccAdapter.for_stage(usd_stage)
scene_info = adapter.get_scene_info()
print(f"Prims: {scene_info.prim_count}")
print(f"Active layer: {scene_info.active_layer}")
```

## Error Handling

```python
from dcc_mcp_core import UsdError

try:
    path = SdfPath.new("invalid/path")  # Must start with /
except UsdError as e:
    print(f"USD error: {e}")

try:
    stage.set_attribute("/NonExistent", "attr", VtValue.int(1))
except UsdError as e:
    print(f"Failed to set attribute: {e}")
```

## Performance Notes

- USDA export is human-readable but slower than JSON
- JSON serialization is compact and fast, ideal for IPC
- Path validation is performed on construction, not lazily
- Large arrays (points, faceConnects) are stored efficiently in Arrow format
