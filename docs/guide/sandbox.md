# Sandbox Guide

Script execution sandbox with API whitelist and audit logging.

## Overview

Enterprise users (game studios, VFX facilities) have strong security requirements that vanilla Python-based DCC MCP integrations cannot satisfy. The sandbox provides:

- **API whitelist / deny list** — Restrict which DCC actions an Agent may invoke
- **Audit log** — Tamper-evident record of every action invocation
- **Input validation** — Schema-based validation before execution
- **Read-only mode** — Query but not mutate the scene
- **Action rate limiting** — Cap actions per session
- **Path allowlist** — File-system access restrictions

## Quick Start

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import json

# Create a restrictive policy
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects", "get_selection"]) \
    .timeout_ms(5000) \
    .build()

# Create sandbox context
ctx = SandboxContext(policy)
ctx = ctx.with_actor("ai-agent")

# Execute an allowed action
result = ctx.execute("get_scene_info", json.dumps({}), None, None)
print(f"Outcome: {result.outcome}")

# Try a forbidden action (will be denied)
result = ctx.execute("delete_all", json.dumps({}), None, None)
print(f"Denied: {result.outcome == 'denied'}")
```

## Policy Configuration

### Allowlist Mode

```python
# Only allow specific actions
policy = SandboxPolicy.builder() \
    .allow_actions([
        "get_scene_info",
        "list_objects",
        "get_selection",
        "query_attributes"
    ]) \
    .build()
```

### Denylist Mode

```python
# Block dangerous actions
policy = SandboxPolicy.builder() \
    .allow_actions(["*"])  # Allow everything except...
    .deny_actions([
        "delete_all",
        "format_disk",
        "run_external_command"
    ]) \
    .build()
```

### Read-Only Mode

```python
# Scene queries only, no mutations
policy = SandboxPolicy.builder() \
    .allow_actions(["get_*", "list_*", "query_*"]) \
    .read_only(True) \
    .build()
```

### Comprehensive Policy

```python
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects"]) \
    .deny_actions(["delete_*"]) \
    .max_actions(100) \
    .timeout_ms(10000) \
    .read_only(False) \
    .allowed_paths(["/project/assets", "/project/scenes"]) \
    .rate_limit(60) \
    .build()
```

## Audit Logging

### Accessing Audit Log

```python
# Execute some actions
ctx.execute("get_scene_info", json.dumps({}), None, None)
ctx.execute("list_objects", json.dumps({}), None, None)

# Get the audit log
log = ctx.audit_log()

print(f"Total actions: {len(log)}")
print(f"Successes: {len(log.successes())}")
print(f"Denials: {len(log.denials())}")
print(f"Failures: {len(log.failures())}")
```

### Audit Entry Details

```python
log = ctx.audit_log()

for entry in log.entries:
    print(f"Time: {entry.timestamp}")
    print(f"Actor: {entry.actor}")
    print(f"Action: {entry.action}")
    print(f"Params: {entry.params}")
    print(f"Outcome: {entry.outcome}")
    print(f"Duration: {entry.duration_ms}ms")
    if entry.error:
        print(f"Error: {entry.error}")
```

### Security Audit

```python
# Find all denied actions
denials = log.denials()
for entry in denials:
    print(f"Suspicious: {entry.actor} tried {entry.action}")

# Find all failures
failures = log.failures()
for entry in failures:
    print(f"Failed: {entry.action} - {entry.error}")
```

## Input Validation

### Defining Schemas

```python
from dcc_mcp_core import InputValidator, FieldSchema, ValidationRule

validator = InputValidator()

# Register a schema for script execution
validator.register(
    "run_script",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.MAX_LENGTH, 10000)
        .rule(ValidationRule.FORBIDDEN_SUBSTRINGS, [
            "__import__",
            "exec(",
            "eval(",
            "subprocess",
            "os.system"
        ])
)

# Register schema for file operations
validator.register(
    "read_file",
    FieldSchema.new()
        .rule(ValidationRule.IS_STRING)
        .rule(ValidationRule.PATTERN, r"^/project/.*")
)
```

### Using Validators

```python
# Safe input
safe_input = {"script": "print('hello world')"}
result = ctx.execute("run_script", json.dumps(safe_input), validator, None)
# Result: success

# Malicious input (blocked by validation)
malicious_input = {"script": "__import__('os').system('rm -rf /')"}
result = ctx.execute("run_script", json.dumps(malicious_input), validator, None)
# Result: validation_failed
```

### Validation Rules

| Rule | Description |
|------|-------------|
| `IS_STRING` | Value must be a string |
| `IS_NUMBER` | Value must be a number |
| `MIN_LENGTH` | String minimum length |
| `MAX_LENGTH` | String maximum length |
| `PATTERN` | Regex pattern match |
| `FORBIDDEN_SUBSTRINGS` | Blocked substrings |
| `ALLOWED_VALUES` | Enum-like values |

## Execution Modes

### Interactive (Default)

```python
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info"]) \
    .timeout_ms(5000) \
    .build()

ctx = SandboxContext(policy)
result = ctx.execute("get_scene_info", json.dumps({}), None, None)
```

### Background

```python
# For long-running operations
policy = SandboxPolicy.builder() \
    .allow_actions(["export_scene", "bake_animation"]) \
    .timeout_ms(300000) \
    .build()
```

### Batch

```python
# Process multiple actions
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects"]) \
    .max_actions(1000) \
    .build()
```

## Error Handling

```python
from dcc_mcp_core import SandboxError

try:
    result = ctx.execute("forbidden_action", json.dumps({}), None, None)
except SandboxError as e:
    print(f"Security error: {e}")
```

## Best Practices

### 1. Start Restrictive

```python
# Start with minimal permissions
policy = SandboxPolicy.builder() \
    .allow_actions([])  # Nothing allowed initially
    .build()

# Add specific actions as needed
policy = policy.add_allowlist(["get_scene_info"])
```

### 2. Separate Read and Write

```python
# Query-only context
read_ctx = SandboxContext(read_policy)
read_ctx = read_ctx.with_actor("query-agent")

# Mutation context
write_ctx = SandboxContext(write_policy)
write_ctx = write_ctx.with_actor("mutation-agent")
```

### 3. Monitor and Alert

```python
# Check for suspicious patterns
log = ctx.audit_log()
denials = [e for e in log.denials() if "delete" in e.action]

if len(denials) > 5:
    alert_security_team(denials)
```

### 4. Time Out Long Operations

```python
# Prevent runaway scripts
policy = SandboxPolicy.builder() \
    .allow_actions(["export_scene"]) \
    .timeout_ms(60000)  # 1 minute max
    .build()
```

## Integration Examples

### Maya Integration

```python
from dcc_mcp_core import SandboxPolicy, SandboxContext
import maya.cmds as cmds
import json

# Create Maya-specific policy
policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene_info", "list_objects", "query_attributes"]) \
    .deny_actions(["delete_*", "set_*", "create_*"]) \
    .read_only(True) \
    .build()

maya_sandbox = SandboxContext(policy)
maya_sandbox = maya_sandbox.with_actor("maya-agent")

# Wrap Maya commands
def safe_maya_action(action, params):
    return maya_sandbox.execute(action, json.dumps(params), None, None)
```

### Blender Integration

```python
import bpy
from dcc_mcp_core import SandboxPolicy, SandboxContext

policy = SandboxPolicy.builder() \
    .allow_actions(["get_scene", "list_objects", "query_data"]) \
    .read_only(True) \
    .build()

blender_sandbox = SandboxContext(policy)
```
