# Actions API

`dcc_mcp_core` — ActionRegistry, EventBus, ActionDispatcher, ActionValidator, VersionedRegistry.

## ActionRegistry

Thread-safe action registry backed by DashMap. Each registry instance is independent.

### Constructor

```python
from dcc_mcp_core import ActionRegistry
registry = ActionRegistry()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `register(name, description="", category="", tags=[], dcc="python", version="1.0.0", input_schema=None, output_schema=None, source_file=None)` | — | Register an action |
| `get_action(name, dcc_name=None)` | `dict?` | Get action metadata as dict |
| `list_actions(dcc_name=None)` | `List[dict]` | List all actions as metadata dicts |
| `list_actions_for_dcc(dcc_name)` | `List[str]` | List action names for a DCC |
| `get_all_dccs()` | `List[str]` | List all registered DCC names |
| `reset()` | — | Clear all registered actions |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__len__` | Number of registered actions |
| `__contains__(name)` | Check if action is registered |
| `__repr__` | `ActionRegistry(actions=N)` |

### Action Metadata Dict

When retrieved via `get_action()` or `list_actions()`, each action is a dict:

```python
{
    "name": "create_sphere",
    "description": "Creates a sphere",
    "category": "geometry",
    "tags": ["geometry"],
    "dcc": "maya",
    "version": "1.0.0",
    "input_schema": {"type": "object", "properties": {}},
    "output_schema": {"type": "object", "properties": {}},
    "source_file": "/path/to/source.py"  # or null
}
```

## ActionValidator

JSON Schema-based input validation for actions.

### Constructor

```python
from dcc_mcp_core import ActionValidator
validator = ActionValidator()
```

### Registering Schemas

```python
# Register a JSON Schema for an action
validator.register_schema(
    "create_sphere",
    {
        "type": "object",
        "properties": {
            "radius": {"type": "number", "minimum": 0},
            "name": {"type": "string"}
        },
        "required": ["radius"]
    }
)
```

### Validating Input

```python
from dcc_mcp_core import ValidationResult

# Valid input
result = validator.validate("create_sphere", {"radius": 1.0, "name": "sphere1"})
print(result.valid)       # True
print(result.action_name) # "create_sphere"

# Invalid input
result = validator.validate("create_sphere", {"radius": -1.0})
print(result.valid)       # False
print(result.errors)      # ["radius must be >= 0"]
```

### ValidationResult

| Field | Type | Description |
|-------|------|-------------|
| `valid` | `bool` | Whether validation passed |
| `action_name` | `str` | Action that was validated |
| `errors` | `List[str]` | List of validation errors |
| `validated_input` | `dict` | Sanitized input dict |

### Error Handling

```python
from dcc_mcp_core import ValidationError

try:
    validator.validate("nonexistent", {})
except ValidationError as e:
    print(f"No schema for action: {e}")
```

## ActionDispatcher

Route actions to handler functions with version compatibility.

### Constructor

```python
from dcc_mcp_core import ActionDispatcher

dispatcher = ActionDispatcher()
```

### Registering Handlers

```python
def handle_create_sphere(ctx, input):
    # Handle the action
    return {"success": True, "sphere": input.get("name", "sphere")}

dispatcher.register_handler("create_sphere", handle_create_sphere)
```

### Dispatching Actions

```python
result = dispatcher.dispatch("create_sphere", context={}, input={"radius": 1.0})
print(result)  # {"success": True, "sphere": "sphere"}
```

### Handler Function Signature

```python
def handler(context: dict, input: dict) -> dict:
    """
    Args:
        context: DCC context information
        input: Validated action input
    Returns:
        Action result dict
    """
    pass
```

## SemVer

Semantic versioning utilities.

### Parsing Versions

```python
from dcc_mcp_core import SemVer

v = SemVer.parse("1.2.3")
print(v.major)  # 1
print(v.minor)  # 2
print(v.patch)  # 3
print(v.prerelease)  # None or "alpha", "beta", "rc.1"
print(v.build_metadata)  # None or "build.123"
```

### Version Comparison

```python
v1 = SemVer.parse("1.2.3")
v2 = SemVer.parse("1.2.4")
v3 = SemVer.parse("2.0.0")

print(v1 < v2)  # True
print(v2 > v1)  # True
print(v3 > v1)  # True
print(v1 == v1)  # True
```

### Version Sorting

```python
versions = [
    SemVer.parse("2.0.0"),
    SemVer.parse("1.0.0"),
    SemVer.parse("1.2.3"),
]
sorted_versions = sorted(versions)
print([str(v) for v in sorted_versions])  # ["1.0.0", "1.2.3", "2.0.0"]
```

### Error Handling

```python
from dcc_mcp_core import VersionParseError

try:
    v = SemVer.parse("invalid")
except VersionParseError as e:
    print(f"Invalid version: {e}")
```

## VersionConstraint

Version requirement specification.

### Creating Constraints

```python
from dcc_mcp_core import VersionConstraint

# Various constraint types
constraint1 = VersionConstraint.parse(">=1.0.0,<2.0.0")
constraint2 = VersionConstraint.parse("^1.2.3")  # Compatible with 1.x.x
constraint3 = VersionConstraint.parse("~1.2.0")  # Roughly equivalent to 1.2.x
constraint4 = VersionConstraint.parse("1.2.3")   # Exact version
```

### Checking Constraints

```python
v = SemVer.parse("1.5.0")
constraint = VersionConstraint.parse(">=1.0.0,<2.0.0")

print(constraint.matches(v))  # True
```

### Supported Constraint Formats

| Format | Example | Description |
|--------|---------|-------------|
| Exact | `1.2.3` | Must match exactly |
| Greater than | `>1.2.3` | Must be greater |
| Range | `>=1.0.0,<2.0.0` | Within range |
| Caret | `^1.2.3` | Compatible (1.x.x) |
| Tilde | `~1.2.3` | Patch compatible (1.2.x) |
| OR | `^1.0.0\|\|^2.0.0` | Either OR |

## VersionedRegistry

Registry with semantic version support for backward compatibility.

### Constructor

```python
from dcc_mcp_core import VersionedRegistry
registry = VersionedRegistry()
```

### Registering Versioned Actions

```python
# Register multiple versions of the same action
registry.register(
    name="create_sphere",
    version="1.0.0",
    handler=handle_v1,
    input_schema={"type": "object", "properties": {"radius": {"type": "number"}}}
)

registry.register(
    name="create_sphere",
    version="2.0.0",
    handler=handle_v2,
    input_schema={"type": "object", "properties": {"radius": {"type": "number"}, "segments": {"type": "integer"}}}
)
```

### Looking Up Actions

```python
# Get latest version
action = registry.get_latest("create_sphere")

# Get specific version
action = registry.get_version("create_sphere", "1.0.0")

# Find compatible version
action = registry.find_compatible("create_sphere", ">=1.0.0,<2.0.0")
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `register(name, version, handler, input_schema=None)` | — | Register a versioned action |
| `get_latest(name)` | `dict?` | Get latest version of an action |
| `get_version(name, version)` | `dict?` | Get specific version |
| `find_compatible(name, constraint)` | `dict?` | Find version matching constraint |
| `list_versions(name)` | `List[str]` | List all versions of an action |

## CompatibilityRouter

Route actions to handlers based on version constraints.

### Constructor

```python
from dcc_mcp_core import CompatibilityRouter

router = CompatibilityRouter()
```

### Registering Routes

```python
router.add_route(
    action="create_sphere",
    constraint=">=1.0.0,<2.0.0",
    handler=handle_v1
)
router.add_route(
    action="create_sphere",
    constraint=">=2.0.0",
    handler=handle_v2
)
```

### Routing Requests

```python
# Route based on client version header
result = router.route(
    action="create_sphere",
    client_version="1.5.0",
    context={},
    input={}
)

# Route based on explicit constraint
result = router.route(
    action="create_sphere",
    constraint=">=1.0.0,<2.0.0",
    context={},
    input={}
)
```

### Fallback Handling

```python
router.add_fallback_handler("create_sphere", fallback_handler)

# If no route matches, fallback is used
result = router.route(action="create_sphere", ...)
```

## DispatchResult

Return type for dispatch operations.

```python
result = dispatcher.dispatch("create_sphere", context, input)

print(result.success)      # True
print(result.action_name)   # "create_sphere"
print(result.version)       # "1.0.0"
print(result.output)        # Handler output
print(result.duration_ms)   # Execution time
```

| Field | Type | Description |
|-------|------|-------------|
| `success` | `bool` | Whether dispatch succeeded |
| `action_name` | `str` | Action that was dispatched |
| `version` | `str?` | Handler version used |
| `output` | `dict` | Handler output |
| `error` | `str?` | Error message if failed |
| `duration_ms` | `int` | Execution time |
