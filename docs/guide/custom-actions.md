# Custom Skills

Learn how to build custom skills for DCC applications — from the recommended Skills-First approach to the low-level registry API.

## Recommended: Skills-First Approach

The Skills-First approach uses `SKILL.md` packages discovered via environment variables. This is the recommended way to build DCC tools because:

- **Zero boilerplate** — no manual handler registration; tools are auto-discovered
- **Auto-exposed as MCP tools** — the skill server exposes each tool to the AI via MCP
- **Hot-reload** — changes to `SKILL.md` are picked up without restart

### Step 1: Create a SKILL.md Package

```markdown
---
name: maya-geometry
description: Maya geometry creation tools
version: 1.0.0
dcc: maya
tags: [geometry, create]
tools:
  - name: create_sphere
    description: Create a polygon sphere
    input_schema: |
      {
        "type": "object",
        "required": ["radius"],
        "properties": {
          "radius": {"type": "number", "minimum": 0.1},
          "name": {"type": "string"}
        }
      }
scripts:
  - create_sphere.py
---

# Maya Geometry Tools

Tools for creating and editing geometry in Maya.
```

### Step 2: Implement the Script

`create_sphere.py`:

```python
import maya.cmds as cmds
from dcc_mcp_core import success_result, error_result


def create_sphere(radius: float = 1.0, name: str | None = None):
    try:
        sphere = cmds.polySphere(r=radius, n=name)[0]
        return success_result(
            message=f"Created sphere: {sphere}",
            context={"object_name": sphere, "radius": radius}
        )
    except Exception as e:
        return error_result(str(e))
```

### Step 3: Register via Environment Variable and Start

```python
import os
from dcc_mcp_core import create_skill_server

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my/skills"

# One call: discovers skills, starts MCP HTTP server
manager = create_skill_server("maya")
```

::: tip Skills-First is the recommended pattern
Use `SKILL.md` packages for all new DCC tools. Fall back to the registry API only when you need programmatic control over handler logic at runtime.
:::

---

## Low-Level Registry API

Use the `ToolRegistry` + `ToolDispatcher` API when you need runtime programmatic control over which handlers are registered.

## Complete Example

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher

# 1. Register tool metadata with a JSON Schema
reg = ToolRegistry()
reg.register(
    name="create_sphere",
    description="Create a polygon sphere in Maya",
    category="geometry",
    tags=["geo", "create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {
            "radius": {
                "type": "number",
                "minimum": 0.1,
                "description": "Radius of the sphere"
            },
            "segments": {
                "type": "integer",
                "minimum": 4,
                "default": 16,
                "description": "Number of subdivisions"
            },
            "name": {
                "type": "string",
                "description": "Optional name for the sphere"
            }
        }
    }),
)

# 2. Create dispatcher and register the handler
dispatcher = ToolDispatcher(reg)

def handle_create_sphere(params):
    radius = params.get("radius", 1.0)
    segments = params.get("segments", 16)
    name = params.get("name")

    # Call Maya API (example using maya.cmds)
    import maya.cmds as cmds
    sphere_name = cmds.polySphere(r=radius, sx=segments, sy=segments, n=name)[0]

    return {
        "created": True,
        "object_name": sphere_name,
        "radius": radius,
        "segments": segments,
    }

dispatcher.register_handler("create_sphere", handle_create_sphere)

# 3. Dispatch from JSON (wire format)
import json
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0, "segments": 32}))
print(result["output"]["object_name"])  # "pSphere1"
```

## Key Points

1. **Register with `ToolRegistry.register()`** — pass name, description, tags, DCC, version, and a JSON Schema
2. **Implement a handler function** — takes `params: dict`, returns a result dict
3. **Register handler with `ToolDispatcher`** — connects the tool name to your Python callable
4. **Use JSON Schema for validation** — `ToolDispatcher` validates JSON input before calling your handler
5. **Dispatch with JSON strings** — the wire format uses JSON, not Python dicts

## Handler Function Signature

```python
def my_handler(params: dict) -> Any:
    """
    Args:
        params: Validated parameters from the JSON input (already parsed)
    Returns:
        A dict with the tool result (serializable to JSON)
    """
    pass
```

## Validation with ToolValidator

Validate input before dispatching:

```python
from dcc_mcp_core import ToolValidator

validator = ToolValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
ok, errors = validator.validate('{"radius": 1.5}')
if not ok:
    print(f"Validation failed: {errors}")
    # Handle error
```

## Versioned Tools

Maintain backward compatibility with `VersionedRegistry`:

```python
from dcc_mcp_core import VersionedRegistry

vr = VersionedRegistry()

# v1: Basic sphere
vr.register_versioned(
    "create_sphere", dcc="maya", version="1.0.0",
    description="Basic sphere creation",
)

# v2: Adds segments parameter
vr.register_versioned(
    "create_sphere", dcc="maya", version="2.0.0",
    description="Sphere with subdivision control",
)

# Auto-resolve best version
result = vr.resolve("create_sphere", "maya", "^1.0.0")
print(result["version"])  # "2.0.0"
```

## JSON Schema Tips

- Use `$ref` for reusable schemas (not supported in ToolValidator — inline all definitions)
- `"default"` field sets default values when key is missing in input
- Use `"minimum"`/`maximum` for numeric constraints
- Use `"minLength"`/`maxLength` for string length
- Use `"enum"` for restricted string choices

```python
input_schema = json.dumps({
    "type": "object",
    "required": ["radius"],
    "properties": {
        "radius": {
            "type": "number",
            "minimum": 0.1,
            "maximum": 1000.0,
        },
        "name": {
            "type": "string",
            "minLength": 1,
            "maxLength": 64,
        },
        "align_to_world": {
            "type": "boolean",
            "default": False,
        }
    }
})
```
