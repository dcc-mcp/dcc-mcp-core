# Custom Actions

Learn how to create custom actions for DCC applications using the registry-based API.

## Complete Example

```python
import json
from dcc_mcp_core import ActionRegistry, ActionDispatcher

# 1. Register action metadata with a JSON Schema
reg = ActionRegistry()
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
dispatcher = ActionDispatcher(reg)

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

1. **Register with `ActionRegistry.register()`** — pass name, description, tags, DCC, version, and a JSON Schema
2. **Implement a handler function** — takes `params: dict`, returns a result dict
3. **Register handler with `ActionDispatcher`** — connects the action name to your Python callable
4. **Use JSON Schema for validation** — `ActionDispatcher` validates JSON input before calling your handler
5. **Dispatch with JSON strings** — the wire format uses JSON, not Python dicts

## Handler Function Signature

```python
def my_handler(params: dict) -> Any:
    """
    Args:
        params: Validated parameters from the JSON input (already parsed)
    Returns:
        A dict with the action result (serializable to JSON)
    """
    pass
```

## Validation with ActionValidator

Validate input before dispatching:

```python
from dcc_mcp_core import ActionValidator

validator = ActionValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
ok, errors = validator.validate('{"radius": 1.5}')
if not ok:
    print(f"Validation failed: {errors}")
    # Handle error
```

## Versioned Actions

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

- Use `$ref` for reusable schemas (not supported in ActionValidator — inline all definitions)
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
