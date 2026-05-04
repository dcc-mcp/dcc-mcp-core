# Custom Skills

Learn how to build custom Skills for DCC applications — from the recommended Skills-First approach to the low-level registry API.

## Recommended: Skills-First Approach

The Skills-First approach discovers `SKILL.md` packages via environment variables and is the recommended way to build DCC tools:

- **Zero boilerplate** — no manual handler registration, tools are auto-discovered
- **Auto-exposed as MCP tools** — the skill manager exposes each tool to AI via the MCP protocol
- **Hot-reload** — changes to `SKILL.md` take effect without restart

### Step 1: Create a SKILL.md Package

```markdown
---
name: maya-geometry
description: Maya geometry creation tools
license: MIT
compatibility: maya>=2022
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: domain
  dcc-mcp.tags: [geometry, create]
  dcc-mcp.tools: tools.yaml
---

# Maya Geometry Tools

A toolset for creating and editing geometry in Maya.
```

`tools.yaml`:

```yaml
tools:
  - name: create_sphere
    description: Create a polygon sphere.
    source_file: scripts/create_sphere.py
    input_schema: |
      {
        "type": "object",
        "required": ["radius"],
        "properties": {
          "radius": {"type": "number", "minimum": 0.1},
          "name": {"type": "string"}
        }
      }
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
            object_name=sphere,
            radius=radius,
        )
    except Exception as e:
        return error_result("Failed to create sphere", str(e))
```

### Step 3: Register via Environment Variable and Start

```python
import os
from dcc_mcp_core import McpHttpConfig, create_skill_server

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/path/to/my/skills"

# One line: auto-discovers skills, starts MCP HTTP server
server = create_skill_server("maya", McpHttpConfig(port=8765))
```

::: tip Skills-First is the recommended pattern
All new DCC tools should use `SKILL.md` packages first. Only fall back to the registry API when you need runtime dynamic control over handler logic.
:::

---

## Low-Level Registry API

When you need programmatic control over handler registration at runtime, use the `ToolRegistry` + `ToolDispatcher` API.

### Full Example

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher

# 1. Register action metadata with JSON Schema
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
                "description": "Sphere radius"
            },
            "segments": {
                "type": "integer",
                "minimum": 4,
                "default": 16,
                "description": "Subdivision segments"
            },
            "name": {
                "type": "string",
                "description": "Optional sphere name"
            }
        }
    }),
)

# 2. Create dispatcher and register handler
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

# 3. Dispatch using JSON (wire format)
import json
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0, "segments": 32}))
print(result["output"]["object_name"])  # "pSphere1"
```

## Key Takeaways

1. **Use `ToolRegistry.register()`** — pass name, description, tags, DCC, version, and JSON Schema
2. **Implement a handler function** — receives `params: dict`, returns a result dictionary
3. **Use `ToolDispatcher` to register handlers** — connects an action name to a Python callable
4. **Use JSON Schema for validation** — `ToolDispatcher` validates JSON input before calling the handler
5. **Dispatch with JSON strings** — the wire format uses JSON, not Python dicts

## Handler Function Signature

```python
def my_handler(params: dict) -> Any:
    """
    Args:
        params: Validated parameters (parsed from JSON input)
    Returns:
        A dictionary to be used as the action result (serializable to JSON)
    """
    pass
```

## Validation with ToolValidator

Validate inputs before dispatching:

```python
from dcc_mcp_core import ToolValidator

validator = ToolValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
ok, errors = validator.validate('{"radius": 1.5}')
if not ok:
    print(f"Validation failed: {errors}")
    # Handle error
```

## Versioned Actions

Use `VersionedRegistry` for backward compatibility:

```python
from dcc_mcp_core import VersionedRegistry

vr = VersionedRegistry()

# v1: basic sphere
vr.register_versioned(
    "create_sphere", dcc="maya", version="1.0.0",
    description="Basic sphere creation",
)

# v2: add subdivision parameter
vr.register_versioned(
    "create_sphere", dcc="maya", version="2.0.0",
    description="Sphere with subdivision control",
)

# Automatically resolve the best version
result = vr.resolve("create_sphere", "maya", "^1.0.0")
print(result["version"])  # "2.0.0"
```

## JSON Schema Tips

- Use `$ref` for reusable schemas (ToolValidator does not support this — inline all definitions)
- `"default"` fields set defaults when keys are missing from input
- Use `"minimum"`/`"maximum"` for numeric constraints
- Use `"minLength"`/`"maxLength"` for string length constraints
- Use `"enum"` to restrict string choices

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
