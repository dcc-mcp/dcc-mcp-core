# Sandbox API

`dcc_mcp_core` (sandbox module)

Script execution sandbox with API whitelist, audit logging, and input validation.

## Overview

Enterprise users (game studios, VFX facilities) have strong security requirements that vanilla Python-based DCC MCP integrations cannot satisfy. The sandbox crate provides:

- **API whitelist / deny list** — restrict which DCC actions an Agent may invoke
- **Audit log** — tamper-evident, structured record of every action invocation
- **Input validation** — schema-based validation of Agent-supplied parameters
- **Read-only mode** — Agent can query but not mutate the scene
- **Action rate limiting** — cap the number of actions per session
- **Path allowlist** — restrict file-system access to project directories

## SandboxContext

Main sandbox execution context.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects"]) \
    .timeout_ms(5000) \
    .build()

ctx = SandboxContext(policy)
ctx = ctx.with_actor("my-agent")
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `with_actor(name)` | `SandboxContext` | Set the actor name for audit |
| `execute(action, params, validator, handler)` | `ExecutionResult` | Execute an action |
| `action_count()` | `int` | Number of actions executed |
| `audit_log()` | `AuditLog` | Get the audit log |
| `reset()` | — | Reset the context |

### Execution

```python
result = ctx.execute(
    "get_scene_info",
    json.dumps({}),
    None,  # No custom validator
    None   # No custom handler
)

print(result.outcome)     # "success" or "denied"
print(result.duration_ms) # Execution time
print(result.error)       # Error message if failed
```

## SandboxPolicy

Security policy configuration.

### Builder

```python
policy = SandboxPolicy.builder() \
    .allow_actions(["get_info", "list_objects"]) \
    .deny_actions(["delete_all", "format_disk"]) \
    .max_actions(10) \
    .timeout_ms(5000) \
    .read_only(True) \
    .build()
```

### Policy Options

| Option | Type | Description |
|--------|------|-------------|
| `allow_actions` | `List[str]` | Whitelist of allowed actions |
| `deny_actions` | `List[str]` | Blacklist of denied actions |
| `max_actions` | `int` | Maximum actions per session |
| `timeout_ms` | `int` | Maximum execution time per action |
| `read_only` | `bool` | If True, deny all write operations |
| `allowed_paths` | `List[str]` | Allowed file system paths |
| `rate_limit` | `int` | Maximum actions per minute |

## AuditLog

Tamper-evident audit trail.

### Methods

```python
log = ctx.audit_log()

print(f"Total entries: {len(log)}")
print(f"Successes: {len(log.successes())}")
print(f"Denials: {len(log.denials())}")
print(f"Failures: {len(log.failures())}")

for entry in log.entries:
    print(f"{entry.timestamp}: {entry.action} - {entry.outcome}")
```

### AuditEntry

Each entry contains:

| Field | Type | Description |
|-------|------|-------------|
| `timestamp` | `datetime` | When the action was attempted |
| `actor` | `str` | Who initiated the action |
| `action` | `str` | Action name |
| `params` | `dict` | Action parameters |
| `outcome` | `AuditOutcome` | success/denied/failed |
| `duration_ms` | `int` | Execution duration |
| `error` | `str?` | Error message if failed |

## InputValidator

Schema-based input validation before action execution.

### Creating a Validator

```python
from dcc_mcp_core import InputValidator, FieldSchema, ValidationRule

validator = InputValidator().register(
    "script",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.FORBIDDEN_SUBSTRINGS, ["__import__", "exec(", "eval("])
)
```

### Validation Rules

| Rule | Description |
|------|-------------|
| `IS_STRING` | Value must be a string |
| `IS_NUMBER` | Value must be a number |
| `IS_BOOL` | Value must be a boolean |
| `MIN_LENGTH` | String minimum length |
| `MAX_LENGTH` | String maximum length |
| `PATTERN` | Regex pattern match |
| `FORBIDDEN_SUBSTRINGS` | Block listed substrings |
| `ALLOWED_VALUES` | Enum-like restriction |

### Using a Validator

```python
malicious = {"script": "__import__('os').system('rm -rf /')"}

try:
    result = ctx.execute("run_script", malicious, validator, None)
except SandboxError as e:
    print(f"Validation failed: {e}")
```

## ExecutionResult

Result of a sandboxed action execution.

```python
result = ctx.execute("list_objects", {}, None, None)

# Attributes
result.outcome    # AuditOutcome enum
result.error       # Error message if failed
result.duration_ms # Execution time in milliseconds
result.output      # Action output data
```

## Error Handling

```python
from dcc_mcp_core import SandboxError

try:
    ctx.execute("forbidden_action", {}, None, None)
except SandboxError as e:
    print(f"Sandbox error: {e}")
    # e.g., "Action 'forbidden_action' is not allowed by policy"
```

## Best Practices

1. **Always use allowlists** — Start with no actions allowed, then explicitly permit safe ones
2. **Set timeouts** — Prevent runaway scripts from hanging the DCC
3. **Enable audit logging** — Keep records for security audits
4. **Use read-only mode** — When only querying data, enable read-only to prevent accidental modifications
5. **Validate all inputs** — Use InputValidator to catch injection attacks before they reach DCC code
