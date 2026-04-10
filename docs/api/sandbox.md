# Sandbox API

`dcc_mcp_core` (sandbox module)

Script execution sandbox with API allowlist, path controls, audit logging, and input validation.

## Overview

Enterprise users (game studios, VFX facilities) have strong security requirements that vanilla Python-based DCC MCP integrations cannot satisfy. The sandbox module provides:

- **API allowlist** — restrict which DCC actions an Agent may invoke
- **Path allowlist** — restrict filesystem access to safe directories
- **Audit log** — structured record of every action invocation (`AuditEntry` / `AuditLog`)
- **Input validation** — field-level rules with injection guards (`InputValidator`)
- **Read-only mode** — Agent can query but not mutate the scene

## SandboxPolicy

Security policy configuration for a sandbox session.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy

policy = SandboxPolicy()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `allow_actions(actions)` | `None` | Restrict execution to only these actions (replaces previous allowlist) |
| `deny_actions(actions)` | `None` | Deny these actions even if on the allowlist |
| `allow_paths(paths)` | `None` | Allow filesystem access inside these directory paths |
| `set_timeout_ms(ms)` | `None` | Set execution timeout in milliseconds |
| `set_max_actions(count)` | `None` | Set max number of actions allowed per session |
| `set_read_only(read_only)` | `None` | Enable/disable read-only mode |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `is_read_only` | `bool` | Whether policy is in read-only mode |

### Example

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "get_object_info"])
policy.deny_actions(["delete_scene", "delete_object"])
policy.allow_paths(["/studio/assets", "/tmp/renders"])
policy.set_timeout_ms(5000)
policy.set_max_actions(100)
policy.set_read_only(False)

print(policy.is_read_only)  # False
```

::: tip Use allowlists, not denylists
Start with **no actions allowed**, then explicitly permit safe ones. This is more secure than listing everything you want to block.
:::

## SandboxContext

Main sandbox execution context. Bundles a `SandboxPolicy` with an `AuditLog` and an action counter.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])

ctx = SandboxContext(policy)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `set_actor(actor)` | `None` | Set caller identity for audit entries |
| `execute_json(action, params_json)` | `str` | Execute action with JSON params, returns JSON result |
| `is_allowed(action)` | `bool` | Check if action is permitted by current policy |
| `is_path_allowed(path)` | `bool` | Check if path is within an allowed directory |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `action_count` | `int` | Number of actions successfully executed |
| `audit_log` | `AuditLog` | The audit log for this context |

### Execution

```python
ctx.set_actor("claude-agent")

# Execute with JSON params — returns JSON string
result = ctx.execute_json("get_scene_info", "{}")
print(result)  # '{"name": "my_scene", "object_count": 42, ...}'

# Check permissions without executing
if ctx.is_allowed("delete_object"):
    ctx.execute_json("delete_object", '{"name": "pSphere1"}')
```

::: warning Errors raise RuntimeError
`execute_json()` raises `RuntimeError` if the action is denied, validation fails, or a sandbox error occurs.
:::

### Audit Log Access

```python
log = ctx.audit_log
print(len(log))  # number of recorded entries

for entry in log.entries():
    print(f"{entry.actor}: {entry.action} → {entry.outcome} ({entry.duration_ms}ms)")

# Filtered views
denied = log.denials()
succeeded = log.successes()

# Serialize to JSON
json_str = log.to_json()
```

## AuditEntry

A single audit record for one action invocation. All attributes are read-only properties.

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `timestamp_ms` | `int` | Unix timestamp in milliseconds |
| `actor` | `str \| None` | Caller identity, or `None` |
| `action` | `str` | Action name |
| `params_json` | `str` | Parameters as JSON string |
| `duration_ms` | `int` | Execution duration in milliseconds |
| `outcome` | `str` | `"success"`, `"denied"`, `"error"`, or `"timeout"` |
| `outcome_detail` | `str \| None` | Denial reason or error message, or `None` |

```python
for entry in ctx.audit_log.entries():
    print(f"[{entry.timestamp_ms}] {entry.actor}: {entry.action}")
    print(f"  params: {entry.params_json}")
    print(f"  outcome: {entry.outcome} ({entry.duration_ms}ms)")
    if entry.outcome_detail:
        print(f"  detail: {entry.outcome_detail}")
```

## AuditLog

Read-only view of the sandbox audit log.

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `entries()` | `list[AuditEntry]` | All recorded entries |
| `successes()` | `list[AuditEntry]` | Entries with outcome `"success"` |
| `denials()` | `list[AuditEntry]` | Entries with outcome `"denied"` |
| `entries_for_action(action)` | `list[AuditEntry]` | Entries for a specific action |
| `to_json()` | `str` | All entries serialized as a JSON array |
| `__len__()` | `int` | Number of entries |

```python
log = ctx.audit_log

print(f"Total: {len(log)}")
print(f"Successes: {len(log.successes())}")
print(f"Denials: {len(log.denials())}")

# All scene_info queries
scene_queries = log.entries_for_action("get_scene_info")

# Export for logging system
import json
entries_json = log.to_json()
```

## InputValidator

Field-level input validator with injection guards. Use to validate parameters before passing to `SandboxContext.execute_json()`.

### Constructor

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `require_string(field, max_length=None, min_length=None)` | `None` | Register a required string field with optional length bounds |
| `require_number(field, min_value=None, max_value=None)` | `None` | Register a required numeric field with optional range |
| `forbid_substrings(field, substrings)` | `None` | Injection guard: field must not contain any listed substring |
| `validate(params_json)` | `tuple[bool, str \| None]` | Validate JSON params; returns `(True, None)` or `(False, error)` |

### Example

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()

# Field constraints
validator.require_string("name", min_length=1, max_length=64)
validator.require_number("radius", min_value=0.01, max_value=1000.0)

# Injection guards
validator.forbid_substrings("script", ["__import__", "exec(", "eval(", "os.system"])

# Valid input
ok, error = validator.validate('{"name": "sphere1", "radius": 2.0}')
print(ok, error)  # True, None

# Injection attempt blocked
ok, error = validator.validate('{"script": "__import__(os)"}')
print(ok, error)  # False, "field 'script' contains forbidden substring '__import__'"

# Runtime error on invalid JSON
try:
    validator.validate("not json")
except RuntimeError as e:
    print(f"Invalid JSON: {e}")
```

::: warning validate() vs ActionValidator
`InputValidator` is for **sandbox field-level rules** (length, range, injection guards).
`ActionValidator` (from the actions module) validates against a **JSON Schema**.
Use `InputValidator` inside a sandbox; use `ActionValidator` at the action dispatch layer.
:::

## Best Practices

1. **Always use allowlists** — Start with no actions allowed, then explicitly permit safe ones
2. **Set timeouts** — Prevent runaway scripts from hanging the DCC
3. **Limit action counts** — `set_max_actions()` prevents runaway agent loops
4. **Enable audit logging** — Always inspect `ctx.audit_log` after a session
5. **Use read-only mode** — Enable when only querying data to prevent accidental mutations
6. **Add injection guards** — Use `InputValidator.forbid_substrings()` for any script/code parameters
7. **Validate paths** — Use `allow_paths()` + `ctx.is_path_allowed()` to prevent path traversal

## Full Example

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext, InputValidator

# Build policy
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "run_script"])
policy.allow_paths(["/studio/assets"])
policy.set_timeout_ms(10000)
policy.set_max_actions(50)

# Build validator for run_script
validator = InputValidator()
validator.require_string("script", max_length=10000)
validator.forbid_substrings("script", [
    "__import__", "exec(", "eval(", "os.system", "subprocess",
])

# Create context
ctx = SandboxContext(policy)
ctx.set_actor("my-ai-agent")

# Execute a safe read query
result = ctx.execute_json("get_scene_info", "{}")
print(result)

# Attempt a potentially dangerous action (blocked by policy)
try:
    ctx.execute_json("delete_scene", "{}")
except RuntimeError as e:
    print(f"Blocked: {e}")

# Review audit log
for entry in ctx.audit_log.entries():
    print(f"{entry.action}: {entry.outcome}")
```
