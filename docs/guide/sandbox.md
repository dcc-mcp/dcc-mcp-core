# Sandbox Guide

Script execution sandbox with API whitelist, audit logging, and input validation.

## Overview

Enterprise users (game studios, VFX facilities) have strong security requirements that vanilla Python-based DCC MCP integrations cannot satisfy. The sandbox provides:

- **API whitelist / deny list** — restrict which DCC actions an Agent may invoke
- **Audit log** — tamper-evident, structured record of every action invocation
- **Input validation** — schema-based validation of Agent-supplied parameters
- **Read-only mode** — Agent can query but not mutate the scene
- **Path allowlist** — restrict file-system access to project directories

## Quick Start

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

# Create a restrictive policy
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])
policy.deny_actions(["delete_all"])
policy.set_timeout_ms(5000)
policy.set_max_actions(100)
policy.set_read_only(False)

# Create sandbox context
ctx = SandboxContext(policy)
ctx.set_actor("ai-agent")

# Execute an allowed action (returns JSON string)
result_json = ctx.execute_json("get_scene_info", "{}")
print(f"Result: {result_json}")

# Try a forbidden action (raises RuntimeError)
try:
    ctx.execute_json("delete_all", "{}")
except RuntimeError as e:
    print(f"Denied: {e}")
```

## SandboxPolicy

Security policy configuration.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy

policy = SandboxPolicy()
```

### Methods

| Method | Description |
|--------|-------------|
| `allow_actions(actions)` | Restrict execution to only the listed actions |
| `deny_actions(actions)` | Deny these actions even if in the whitelist |
| `allow_paths(paths)` | Allow file-system access inside these directory paths |
| `set_timeout_ms(ms)` | Set the execution timeout in milliseconds |
| `set_max_actions(count)` | Set the maximum number of actions allowed per session |
| `set_read_only(read_only)` | Enable (`True`) or disable read-only mode |
| `is_read_only` | Property: `True` if the policy is in read-only mode |

### Example

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "get_selection"])
policy.deny_actions(["delete_*", "format_*"])
policy.allow_paths(["/project/assets", "/project/scenes"])
policy.set_timeout_ms(10000)
policy.set_max_actions(100)
policy.set_read_only(False)
```

## SandboxContext

Main sandbox execution context for a single session.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy()
policy.allow_actions(["echo"])
ctx = SandboxContext(policy)
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `set_actor(actor)` | `None` | Set the caller identity for audit entries |
| `execute_json(action, params_json)` | `str` | Execute an action; returns result as JSON string |
| `is_allowed(action)` | `bool` | Check if `action` is permitted by the policy |
| `is_path_allowed(path)` | `bool` | Check if `path` is within an allowed directory |
| `action_count` | `int` | Number of actions successfully executed (property) |
| `audit_log` | `AuditLog` | The audit log for this context (property) |

### execute_json

Runs the full sandbox pipeline (policy check, validation). Returns result as a JSON string:

```python
ctx = SandboxContext(policy)
ctx.set_actor("my-agent")

# Execute with JSON parameters
result_json = ctx.execute_json("echo", '{"x": 1}')
# Returns: '{"success": true, ...}'

# Execute with empty params
result_json = ctx.execute_json("echo", "{}")

# Denied action raises RuntimeError
try:
    ctx.execute_json("forbidden_action", "{}")
except RuntimeError as e:
    print(f"Sandbox error: {e}")
```

## Audit Logging

### Accessing the Audit Log

```python
ctx = SandboxContext(policy)
ctx.set_actor("agent-1")

# Execute some actions
ctx.execute_json("get_scene_info", "{}")
ctx.execute_json("list_objects", "{}")

# Access audit log
log = ctx.audit_log

print(f"Total entries: {len(log)}")
print(f"Successes: {len(log.successes())}")
print(f"Denials: {len(log.denials())}")
```

### Iterating Entries

```python
log = ctx.audit_log

for entry in log.entries():
    print(f"Time: {entry.timestamp_ms}")
    print(f"Actor: {entry.actor}")
    print(f"Action: {entry.action}")
    print(f"Params: {entry.params_json}")
    print(f"Duration: {entry.duration_ms}ms")
    print(f"Outcome: {entry.outcome}")  # "success", "denied", "error", "timeout"
    if entry.outcome_detail:
        print(f"Detail: {entry.outcome_detail}")
```

### Filtering

```python
# All entries for a specific action
for entry in log.entries_for_action("get_scene_info"):
    print(entry.outcome)

# Only successes
for entry in log.successes():
    print(f"{entry.action}: {entry.outcome}")

# Only denials (security-relevant)
for entry in log.denials():
    print(f"SUSPICIOUS: {entry.actor} tried {entry.action}")

# Serialize all to JSON
json_str = log.to_json()
```

## AuditEntry

Properties of each entry:

| Property | Type | Description |
|----------|------|-------------|
| `timestamp_ms` | `int` | Unix timestamp in milliseconds |
| `actor` | `str \| None` | Caller identity set via `set_actor()` |
| `action` | `str` | Action name invoked |
| `params_json` | `str` | Parameters as a JSON string |
| `duration_ms` | `int` | Execution duration in milliseconds |
| `outcome` | `str` | `"success"`, `"denied"`, `"error"`, or `"timeout"` |
| `outcome_detail` | `str \| None` | Denial reason or error message |

## Input Validation

Use `InputValidator` to validate action parameters before execution.

### Creating a Validator

```python
from dcc_mcp_core import InputValidator

v = InputValidator()

# Register a required string field with max length
v.require_string("name", max_length=50)

# Register a required numeric field with range
v.require_number("count", min_value=0, max_value=1000)

# Block injection patterns
v.forbid_substrings("script", ["__import__", "exec(", "eval(", "subprocess"])
```

### Validating Input

```python
# Safe input
ok, error = v.validate('{"name": "sphere", "count": 5}')
assert ok, error  # (True, None)

# String too long
ok, error = v.validate('{"name": "x" * 100, "count": 5}')
assert not ok  # (False, "...")

# Injection attempt blocked
ok, error = v.validate('{"script": "__import__('os').system('ls')"}')
assert not ok  # (False, "...")
```

### validate() Return Value

Returns `(True, None)` on success, `(False, error_message)` on failure.

```python
v = InputValidator()
v.require_string("name")

ok, err = v.validate('{"name": "ok"}')
assert ok and err is None

ok, err = v.validate('{"name": 123}')  # wrong type
assert not ok and err is not None
```

## Execution Modes

### Interactive (Default)

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info"])
policy.set_timeout_ms(5000)

ctx = SandboxContext(policy)
result = ctx.execute_json("get_scene_info", "{}")
```

### Read-Only Mode

Prevents any write operations:

```python
policy = SandboxPolicy()
policy.allow_actions(["get_*", "list_*", "query_*"])
policy.set_read_only(True)

ctx = SandboxContext(policy)
# All mutations will be denied
```

## Error Handling

`execute_json()` raises `RuntimeError` on denial, validation failure, or timeout:

```python
try:
    ctx.execute_json("forbidden_action", "{}")
except RuntimeError as e:
    print(f"Sandbox error: {e}")
    # e.g., "Action 'forbidden_action' is not allowed by policy"
```

## Best Practices

### 1. Start Restrictive

```python
# Start with minimal permissions
policy = SandboxPolicy()
policy.allow_actions([])  # Nothing allowed initially

# Add specific actions as needed
policy.allow_actions(["get_scene_info"])
```

### 2. Separate Read and Write Contexts

```python
# Query-only context
read_policy = SandboxPolicy()
read_policy.allow_actions(["get_*", "list_*", "query_*"])
read_policy.set_read_only(True)
read_ctx = SandboxContext(read_policy)
read_ctx.set_actor("query-agent")

# Mutation context
write_policy = SandboxPolicy()
write_policy.allow_actions(["get_*", "list_*", "create_*", "set_*"])
write_ctx = SandboxContext(write_policy)
write_ctx.set_actor("mutation-agent")
```

### 3. Monitor and Alert

```python
log = ctx.audit_log
denials = [e for e in log.denials() if "delete" in e.action]

if len(denials) > 5:
    alert_security_team(denials)
```

### 4. Set Appropriate Timeouts

```python
# Short for quick queries
policy.set_timeout_ms(1000)

# Longer for export/bake operations
policy.set_timeout_ms(300000)
```

## Integration Examples

### Maya Integration

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

# Create Maya-specific policy
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects", "query_attributes"])
policy.deny_actions(["delete_*", "set_*", "create_*"])
policy.set_read_only(True)

maya_sandbox = SandboxContext(policy)
maya_sandbox.set_actor("maya-agent")

# Wrap Maya commands
def safe_maya_action(action, params):
    result_json = maya_sandbox.execute_json(action, json.dumps(params))
    return json.loads(result_json)

scene_info = safe_maya_action("get_scene_info", {})
```

### Blender Integration

```python
import bpy
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy()
policy.allow_actions(["get_scene", "list_objects", "query_data"])
policy.set_read_only(True)

blender_sandbox = SandboxContext(policy)
```
