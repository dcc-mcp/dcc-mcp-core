# Actions API

`dcc_mcp_core` — ActionRegistry, EventBus, ActionDispatcher, ActionValidator, SemVer, VersionConstraint, VersionedRegistry.

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
| `search_actions(category=None, tags=[], dcc_name=None)` | `List[dict]` | Search with AND-ed filters |
| `get_categories(dcc_name=None)` | `List[str]` | Sorted unique categories |
| `get_tags(dcc_name=None)` | `List[str]` | Sorted unique tags |
| `count_actions(category=None, tags=[], dcc_name=None)` | `int` | Count matching actions |
| `reset()` | — | Clear all registered actions |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__len__` | Number of registered actions |
| `__contains__(name)` | Check if action name is registered (scoped to "python" dcc) |
| `__repr__` | `ActionRegistry(actions=N)` |

### Action Metadata Dict

When retrieved via `get_action()`, `list_actions()`, or `search_actions()`, each action is a dict:

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

### Example

```python
reg = ActionRegistry()
reg.register(
    "create_sphere",
    description="Create a polygon sphere",
    category="geometry",
    tags=["geo", "create"],
    dcc="maya",
    input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}',
)

# Get it back
meta = reg.get_action("create_sphere", dcc_name="maya")
print(meta["version"])  # "1.0.0"

# Search
results = reg.search_actions(category="geometry", tags=["create"])
```

## ActionValidator

Validates JSON-encoded action parameters against a JSON Schema. Created from a schema string or from an `ActionRegistry` action.

### Static Factory Methods

```python
from dcc_mcp_core import ActionValidator

# From a JSON Schema string
validator = ActionValidator.from_schema_json(
    '{"type": "object", "required": ["radius"], '
    '"properties": {"radius": {"type": "number", "minimum": 0.0}}}'
)

# From an ActionRegistry action
from dcc_mcp_core import ActionRegistry
reg = ActionRegistry()
reg.register("create_sphere", input_schema='{"type": "object", "properties": {"radius": {"type": "number"}}}')
validator = ActionValidator.from_action_registry(reg, "create_sphere")
```

### Validating Input

```python
# Valid input — returns (True, [])
ok, errors = validator.validate('{"radius": 1.0}')
print(ok)      # True
print(errors)  # []

# Invalid input — returns (False, [error1, ...])
ok, errors = validator.validate('{"radius": -1.0}')
print(ok)      # False
print(errors)  # ["radius must be >= 0"]

# Missing required field
ok, errors = validator.validate("{}")
print(ok)      # False
print(errors)  # ["radius is required"]
```

### Error Handling

```python
try:
    validator.validate('not json at all')
except ValueError as e:
    print(f"Invalid JSON: {e}")
```

::: tip
`validate()` accepts a **JSON string** (`'{"radius": 1.0}'`), not a Python dict. This matches the wire-format used by the MCP protocol.
:::

## ActionDispatcher

Routes action calls to registered Python callables with automatic validation.

### Constructor

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher

reg = ActionRegistry()
dispatcher = ActionDispatcher(reg)
```

### Registering Handlers

```python
def handle_create_sphere(params):
    # params is a dict deserialised from the JSON input
    return {"created": True, "radius": params.get("radius", 1.0)}

dispatcher.register_handler("create_sphere", handle_create_sphere)
```

### Dispatching Actions

```python
import json

result = dispatcher.dispatch("create_sphere", json.dumps({"radius": 2.0}))
# result = {"action": "create_sphere", "output": {"created": True, "radius": 2.0}, "validation_skipped": False}
print(result["output"]["created"])  # True
```

### Handler Function Signature

```python
def handler(params: dict) -> Any:
    """Receive validated JSON params as a Python dict."""
    pass
```

### Other Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `register_handler(action_name, handler)` | — | Register a Python callable |
| `remove_handler(action_name)` | `bool` | Remove handler, return True if existed |
| `has_handler(action_name)` | `bool` | Check if handler is registered |
| `handler_count()` | `int` | Number of registered handlers |
| `handler_names()` | `List[str]` | Alphabetically sorted handler names |
| `skip_empty_schema_validation` | `bool` | Property: skip validation when schema is `{}` |

## SemVer

Semantic versioning with major.minor.patch components. Pre-release labels (`-alpha`, `-beta`) are **stripped and ignored** for all comparisons.

### Constructor

```python
from dcc_mcp_core import SemVer

v = SemVer(1, 2, 3)
print(str(v))  # "1.2.3"
```

### Parsing

```python
from dcc_mcp_core import SemVer

v = SemVer.parse("1.2.3")
print(v.major)  # 1
print(v.minor)  # 2
print(v.patch)  # 3

# Leading "v" is accepted
v2 = SemVer.parse("v2.0")
print(v2.major)  # 2
```

### Version Comparison

```python
v1 = SemVer.parse("1.2.3")
v2 = SemVer.parse("1.2.4")
v3 = SemVer.parse("2.0.0")

print(v1 < v2)   # True
print(v2 > v1)   # True
print(v3 > v1)   # True
print(v1 == SemVer.parse("1.2.3"))  # True
```

### Version Sorting

```python
versions = [SemVer.parse("2.0.0"), SemVer.parse("1.0.0"), SemVer.parse("1.2.3")]
sorted_versions = sorted(versions)
print([str(v) for v in sorted_versions])  # ["1.0.0", "1.2.3", "2.0.0"]
```

### Error Handling

```python
try:
    v = SemVer.parse("invalid")
except ValueError as e:
    print(f"Invalid version: {e}")
```

::: tip
`SemVer` only has three numeric components (`major`, `minor`, `patch`). Pre-release labels and build metadata are stripped and ignored.
:::

## VersionConstraint

Version requirement specification for matching against registered action versions.

### Creating Constraints

```python
from dcc_mcp_core import VersionConstraint

# Various constraint types
constraint1 = VersionConstraint.parse(">=1.0.0,<2.0.0")
constraint2 = VersionConstraint.parse("^1.2.3")  # Compatible with 1.x.x
constraint3 = VersionConstraint.parse("~1.2.3")  # Patch compatible (1.2.x)
constraint4 = VersionConstraint.parse("1.2.3")   # Exact version
constraint5 = VersionConstraint.parse("*")       # Any version
```

### Checking Constraints

```python
from dcc_mcp_core import SemVer, VersionConstraint

v = SemVer.parse("1.5.0")
constraint = VersionConstraint.parse(">=1.0.0,<2.0.0")
print(constraint.matches(v))  # True
```

### Supported Constraint Formats

| Format | Example | Description |
|--------|---------|-------------|
| Exact | `1.2.3` | Must match exactly |
| Greater than | `>1.2.3` | Must be strictly greater |
| Range | `>=1.0.0,<2.0.0` | Within range |
| Caret | `^1.2.3` | Same major (1.x.x) |
| Tilde | `~1.2.3` | Same major.minor (1.2.x) |
| Wildcard | `*` | Any version |

## VersionedRegistry

Multi-version action registry. Allows multiple versions of the same `(action_name, dcc_name)` pair to coexist. Provides resolution of the best-matching version given a constraint.

### Constructor

```python
from dcc_mcp_core import VersionedRegistry
registry = VersionedRegistry()
```

### Registering Versions

```python
registry.register_versioned(
    "create_sphere",
    dcc="maya",
    version="1.0.0",
    description="Create a sphere",
    category="geometry",
    tags=["geo", "create"],
)

registry.register_versioned(
    "create_sphere",
    dcc="maya",
    version="2.0.0",
    description="Create a sphere with segments",
    category="geometry",
    tags=["geo", "create"],
)

registry.register_versioned(
    "create_sphere",
    dcc="blender",
    version="1.0.0",
    description="Blender sphere creation",
)
```

### Resolving Versions

```python
# Get all registered versions for (name, dcc)
versions = registry.versions("create_sphere", "maya")
print(versions)  # ["1.0.0", "2.0.0"]

# Get the latest version string
latest = registry.latest_version("create_sphere", "maya")
print(latest)  # "2.0.0"

# Resolve best match for a constraint — returns metadata dict or None
result = registry.resolve("create_sphere", "maya", "^1.0.0")
if result:
    print(result["version"])   # "2.0.0"
    print(result["category"])  # "geometry"

# Resolve all versions matching a constraint
all_matches = registry.resolve_all("create_sphere", "maya", ">=1.0.0,<3.0.0")
for m in all_matches:
    print(m["version"])  # ["1.0.0", "2.0.0"]
```

### Registry Introspection

```python
# All registered (name, dcc) keys
keys = registry.keys()
print(keys)  # [("create_sphere", "maya"), ("create_sphere", "blender")]

# Total number of versioned entries
print(registry.total_entries())  # 3

# Remove versions by constraint
removed = registry.remove("create_sphere", "maya", "^1.0.0")
print(removed)  # 2 (removed 1.0.0 and 2.0.0)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `register_versioned(name, dcc, version, description, category, tags)` | — | Register an action version |
| `versions(name, dcc)` | `List[str]` | All versions sorted ascending |
| `latest_version(name, dcc)` | `str?` | Highest version string or None |
| `resolve(name, dcc, constraint)` | `dict?` | Best match metadata dict or None |
| `resolve_all(name, dcc, constraint)` | `List[dict]` | All matching metadata dicts |
| `keys()` | `List[tuple]` | All `(name, dcc)` pairs |
| `total_entries()` | `int` | Total entry count across all |
| `remove(name, dcc, constraint)` | `int` | Remove count (by constraint) |

::: tip
`resolve()` and `resolve_all()` use `VersionConstraint.parse()` internally — pass a constraint string like `"^1.0.0"` or `">=1.0.0,<2.0.0"`.
:::


## ActionPipeline

Middleware wrapper around `ActionDispatcher`. Layers logging, timing, audit, and rate-limit middleware in a composable pipeline.

### Constructor

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
dispatcher = ActionDispatcher(reg)
pipeline = ActionPipeline(dispatcher)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `dispatch(action, params_json)` | `dict` | Dispatch through all middleware layers |
| `register_handler(name, fn)` | — | Register a Python handler (mirrors `ActionDispatcher`) |
| `add_logging(log_params=False)` | — | Add trace logging middleware |
| `add_timing()` | `TimingMiddleware` | Add latency tracking; returns handle |
| `add_audit(record_params=False)` | `AuditMiddleware` | Add audit log; returns handle |
| `add_rate_limit(max_calls, window_ms)` | `RateLimitMiddleware` | Add rate limiter; returns handle |
| `add_callable(before_fn, after_fn)` | — | Add Python callable hooks |
| `middleware_count()` | `int` | Number of registered middleware layers |
| `middleware_names()` | `List[str]` | Names in pipeline order |
| `handler_count()` | `int` | Number of registered handlers |

### dispatch() Result

`dispatch()` returns a dict with:

| Key | Type | Description |
|-----|------|-------------|
| `action` | `str` | Action name |
| `output` | `Any` | Handler return value |
| `success` | `bool` | `True` if no exception |
| `error` | `str?` | Error message if failed |
| `validation_skipped` | `bool` | Whether JSON schema validation ran |

### TimingMiddleware

```python
timing = pipeline.add_timing()
pipeline.dispatch("my_action", '{}')

ms = timing.last_elapsed_ms("my_action")  # int | None
```

### AuditMiddleware

```python
audit = pipeline.add_audit(record_params=True)
pipeline.dispatch("my_action", '{}')

records = audit.records()                        # all records
records = audit.records_for_action("my_action")  # filtered
count = audit.record_count()                     # int
audit.clear()
```

Each record dict: `action` (str), `success` (bool), `error` (str | None), `timestamp_ms` (int).

| Method | Returns | Description |
|--------|---------|-------------|
| `records()` | `List[dict]` | All audit records |
| `records_for_action(name)` | `List[dict]` | Records for a specific action |
| `record_count()` | `int` | Total record count |
| `clear()` | — | Remove all records |

### RateLimitMiddleware

Fixed-window rate limiter. Raises `RuntimeError` when `max_calls` is exceeded within `window_ms`.

```python
rl = pipeline.add_rate_limit(max_calls=10, window_ms=1000)
print(rl.call_count("my_action"))  # calls in current window
print(rl.max_calls)                # 10
print(rl.window_ms)                # 1000
```

### Full Example

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, ActionPipeline

reg = ActionRegistry()
reg.register("process_mesh", description="Process mesh", category="geometry")
dispatcher = ActionDispatcher(reg)
dispatcher.register_handler("process_mesh", lambda p: {"vertices": 1024})

pipeline = ActionPipeline(dispatcher)
pipeline.add_logging(log_params=True)
timing = pipeline.add_timing()
audit = pipeline.add_audit(record_params=True)
rl = pipeline.add_rate_limit(max_calls=100, window_ms=60000)

result = pipeline.dispatch("process_mesh", '{"mesh_name": "cube"}')
print(result["output"])                          # {"vertices": 1024}
print(timing.last_elapsed_ms("process_mesh"))    # e.g. 12
print(audit.record_count())                      # 1
```
