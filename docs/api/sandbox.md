# Sandbox API

`dcc_mcp_core` (sandbox module)

Script execution sandbox with API whitelist, audit logging, and input validation.

## Overview

Enterprise users (game studios, VFX facilities) have strong security requirements that vanilla Python-based DCC MCP integrations cannot satisfy. The sandbox crate provides:

- **API whitelist** — restrict which DCC actions an Agent may invoke
- **Audit log** — structured record of every action invocation
- **Input validation** — schema-based validation of Agent-supplied parameters
- **Read-only mode** — Agent can query but not mutate the scene

## SandboxPolicy

Security policy configuration.

### Constructor

```python
from dcc_mcp_core import SandboxPolicy

policy = SandboxPolicy()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `allow_actions(actions)` | `None` | Set list of allowed actions |
| `deny_actions(actions)` | `None` | Set list of denied actions |
| `set_read_only(read_only)` | `None` | Enable read-only mode |
| `set_timeout_ms(timeout_ms)` | `None` | Set execution timeout |

### Example

```python
policy = SandboxPolicy()
policy.allow_actions(["get_scene_info", "list_objects"])
policy.set_read_only(True)
policy.set_timeout_ms(5000)
```

## SandboxContext

Main sandbox execution context.

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
| `set_actor(name)` | `None` | Set the actor name for audit |
| `execute_json(action, params_json, validator=None)` | `str` | Execute an action with JSON params |
| `audit_log()` | `list[dict]` | Get the audit log |

### Execution

```python
ctx.set_actor("my-agent")

# Execute with JSON params
result = ctx.execute_json("get_scene_info", "{}")
print(result)

# With custom validator
result = ctx.execute_json("run_script", '{"script": "print(1)"}', validator=validator)
```

### Audit Log

```python
log = ctx.audit_log()

for entry in log:
    print(f"Actor: {entry['actor']}")
    print(f"Action: {entry['action']}")
    print(f"Outcome: {entry['outcome']}")
```

## InputValidator

Schema-based input validation before action execution.

### Constructor

```python
from dcc_mcp_core import InputValidator

validator = InputValidator()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `set_rules(rules)` | `None` | Set validation rules per action |
| `add_forbidden_patterns(action, patterns)` | `None` | Add forbidden patterns |

### Example

```python
validator = InputValidator()
validator.set_rules([
    {"action": "run_script", "max_length": 10000},
])
validator.add_forbidden_patterns("run_script", [
    "__import__",
    "exec(",
    "eval(",
])
```

### Using Validator

```python
# Safe input
result = ctx.execute_json("run_script", '{"script": "print(1)"}', validator=validator)

# Malicious input (blocked)
result = ctx.execute_json("run_script", '{"script": "__import__"}', validator=validator)
```

## Best Practices

1. **Always use allowlists** — Start with no actions allowed, then explicitly permit safe ones
2. **Set timeouts** — Prevent runaway scripts from hanging the DCC
3. **Enable audit logging** — Keep records for security audits
4. **Use read-only mode** — When only querying data, enable read-only to prevent accidental modifications
5. **Validate all inputs** — Use InputValidator to catch injection attacks before they reach DCC code
