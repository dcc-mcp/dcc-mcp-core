# Skills

Skills are the core building blocks of DCC-MCP-Core. Each skill represents a discrete operation that can be performed in a DCC application (Maya, Blender, Houdini, etc.).

## Architecture

DCC-MCP-Core uses a registry-based skill execution model backed by Rust's DashMap for thread-safe, concurrent access:

- **`ToolRegistry`** — Thread-safe store for skill metadata (name, description, tags, DCC, version, JSON schemas)
- **`ToolDispatcher`** — Routes validated calls to registered Python handlers
- **`ToolValidator`** — JSON Schema-based input validation
- **`VersionedRegistry`** — Multi-version skill support with semantic version resolution

All skills are discovered and registered at runtime. There are **no base classes or Pydantic models** — skills are plain Python functions registered with metadata.

## ToolRegistry

`ToolRegistry` is the central registry for all DCC skill operations. Register a skill with a JSON Schema for input validation:

```python
import json
from dcc_mcp_core import ToolRegistry

reg = ToolRegistry()

reg.register(
    name="create_sphere",
    description="Create a polygon sphere in the DCC scene",
    category="geometry",
    tags=["geo", "create", "mesh"],
    dcc="maya",
    version="1.0.0",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {
            "radius": {"type": "number", "minimum": 0.1, "description": "Sphere radius"},
            "segments": {"type": "integer", "minimum": 4, "default": 16},
            "name": {"type": "string", "description": "Optional sphere name"}
        }
    }),
)
```

### Discovery and Lookup

```python
# Get all DCCs that have registered skills
dccs = reg.get_all_dccs()
print(dccs)  # ["maya", "blender", "houdini"]

# List all skills for Maya
maya_skills = reg.list_actions_for_dcc("maya")
print(maya_skills)  # ["create_sphere", "create_cube", ...]

# Get full metadata
meta = reg.get_action("create_sphere", dcc_name="maya")
print(meta["version"])  # "1.0.0"

# Search by category and tags
results = reg.search_actions(category="geometry", tags=["create"])
for r in results:
    print(r["name"], r["dcc"])

# All categories and tags
categories = reg.get_categories()
tags = reg.get_tags(dcc_name="maya")
```

### Dunder Access

```python
reg.register("echo", dcc="python")
print("echo" in reg)  # True
print(len(reg))        # Number of registered skills
```

## ToolDispatcher

`ToolDispatcher` pairs with `ToolRegistry` to provide validated, routed skill execution:

```python
import json
from dcc_mcp_core import ToolRegistry, ToolDispatcher

reg = ToolRegistry()
reg.register(
    "create_sphere",
    dcc="maya",
    input_schema=json.dumps({
        "type": "object",
        "required": ["radius"],
        "properties": {"radius": {"type": "number"}}
    }),
)
dispatcher = ToolDispatcher(reg)

def handle_create_sphere(params):
    radius = params["radius"]
    # Call Maya API here (e.g., via pymel or maya.cmds)
    return {"created": True, "radius": radius}

dispatcher.register_handler("create_sphere", handle_create_sphere)

# Dispatch with JSON string (wire format)
result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result = {"action": "create_sphere", "output": {"created": True, "radius": 2.0}, "validation_skipped": False}
```

## ToolValidator

Standalone validator for checking JSON params against a schema:

```python
from dcc_mcp_core import ToolValidator

validator = ToolValidator.from_schema_json(
    '{"type": "object", "required": ["radius"], '
    '"properties": {"radius": {"type": "number", "minimum": 0}}}'
)

ok, errors = validator.validate('{"radius": 1.5}')
print(ok, errors)  # True, []

ok, errors = validator.validate('{"radius": -1}')
print(ok, errors)  # False, ["radius must be >= 0"]
```

Or create from an existing `ToolRegistry` action:

```python
from dcc_mcp_core import ToolRegistry, ToolValidator

reg = ToolRegistry()
reg.register("create_sphere", dcc="maya", input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}')

validator = ToolValidator.from_action_registry(reg, "create_sphere", dcc_name="maya")
```

## Result Models

All skill execution results normalize to `ToolResult`:

```python
from dcc_mcp_core import success_result, error_result, from_exception

# Success
result = success_result(
    message="Sphere created",
    prompt="Consider adding materials",  # AI guidance
    object_name="sphere1",
    position=[0, 0, 0],
)
print(result.success)    # True
print(result.prompt)    # "Consider adding materials"
print(result.context)   # {"object_name": "sphere1", "position": [0, 0, 0]}

# Error
result = error_result(
    message="Failed to create sphere",
    error="Maya API error: object already exists",
    object_name="sphere1",
)
print(result.success)  # False
print(result.error)    # "Maya API error: object already exists"

# From exception
try:
    raise RuntimeError("connection refused")
except Exception:
    result = from_exception("Connection to Maya lost")
    print(result.success)  # False
```

## VersionedRegistry

For APIs that need to maintain backward compatibility across multiple skill versions:

```python
from dcc_mcp_core import VersionedRegistry, VersionConstraint

vr = VersionedRegistry()

# Register multiple versions of the same skill
vr.register_versioned("create_sphere", dcc="maya", version="1.0.0",
    description="Basic sphere creation", category="geometry", tags=["geo"])
vr.register_versioned("create_sphere", dcc="maya", version="2.0.0",
    description="Sphere with UV support", category="geometry", tags=["geo", "uv"])

# Resolve best version
result = vr.resolve("create_sphere", "maya", "^1.0.0")
print(result["version"])   # "2.0.0"

# All matching versions
all_v = vr.resolve_all("create_sphere", "maya", ">=1.0.0")
print([v["version"] for v in all_v])  # ["1.0.0", "2.0.0"]

# Latest
print(vr.latest_version("create_sphere", "maya"))  # "2.0.0"
```

## EventBus

Subscribe to skill execution lifecycle events for monitoring, logging, or chaining:

```python
from dcc_mcp_core import EventBus

bus = EventBus()

def on_before_execute(event, **kwargs):
    print(f"Executing {event} with {kwargs}")

def on_after_execute(event, **kwargs):
    print(f"Completed {event}")

# Subscribe to all "before_execute" events (wildcard)
id1 = bus.subscribe("action.before_execute.*", on_before_execute)

# Subscribe to a specific action
id2 = bus.subscribe("action.after_execute.create_sphere", on_after_execute)

# Unsubscribe
bus.unsubscribe("action.before_execute.*", id1)

# Publish manually
bus.publish("custom.event", custom_data="value")
```
