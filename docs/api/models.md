# Models

## ToolResult

Standardized result for all action executions. Backed by a Rust struct via PyO3.

### Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `success` | `bool` | `True` | Whether the execution was successful |
| `message` | `str` | `""` | Human-readable result description |
| `prompt` | `Optional[str]` | `None` | Suggestion for AI about next steps |
| `error` | `Optional[str]` | `None` | Error message when `success` is `False` |
| `context` | `Dict[str, Any]` | `{}` | Additional context data |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `with_error(error)` | `ToolResult` | Create copy with error info (sets `success=False`) |
| `with_context(**kwargs)` | `ToolResult` | Create copy with updated context |
| `to_dict()` | `Dict[str, Any]` | Convert to dictionary |
| `to_json()` | `str` | Serialize to a JSON string |
| `__eq__(other)` | `bool` | Equality comparison |
| `__str__()` | `str` | Human-readable string |
| `__repr__()` | `str` | Unambiguous representation |

::: warning `json.dumps()` is not supported directly
`ToolResult` is a Rust-backed object and **cannot be passed to `json.dumps()` directly**.
Use `to_json()` or convert to a dict first:

```python
import json
result = success_result("done")

# Option 1 — built-in JSON serializer (recommended, uses Rust serde)
json_str = result.to_json()

# Option 2 — via dict
json_str = json.dumps(result.to_dict())

# Option 3 — serialize_result (supports JSON and MsgPack)
from dcc_mcp_core import serialize_result
json_str = serialize_result(result)
```
:::

### Factory Functions

```python
from dcc_mcp_core import success_result, error_result, from_exception, validate_action_result, ToolResult

# Success result with context
result = success_result("Created 5 spheres", prompt="Use modify_spheres next", count=5)

# Error result with possible solutions
error = error_result(
    "Failed", "File not found",
    prompt="Check path",
    possible_solutions=["Verify file exists", "Check permissions"],
    path="/bad/path",
)

# From exception string
exc_result = from_exception(
    "ValueError: bad input",
    message="Import failed",
    include_traceback=True,
)

# Validate/normalize any value to ToolResult
validate_action_result(result)                          # pass-through
validate_action_result({"success": True, "message": "OK"})  # dict → ARM
validate_action_result("hello")                         # wrap as success
```

### Factory Function Signatures

| Function | Signature | Description |
|----------|-----------|-------------|
| `success_result` | `(message, prompt=None, **context) -> ToolResult` | Create a successful result |
| `error_result` | `(message, error, prompt=None, possible_solutions=None, **context) -> ToolResult` | Create a failed result |
| `from_exception` | `(error_message, message=None, prompt=None, include_traceback=True, possible_solutions=None, **context) -> ToolResult` | Wrap an exception as a result |
| `validate_action_result` | `(result: Any) -> ToolResult` | Normalize dict/str/None/ToolResult → ToolResult |

## SkillMetadata

Metadata parsed from SKILL.md frontmatter. All fields are readable and writable.

### Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | — | Unique identifier |
| `description` | `str` | `""` | Human-readable description |
| `search_hint` | `str` | `""` | Keyword hint for `search_skills` (SKILL.md `search-hint:` field; falls back to `description`) |
| `tools` | `List[str]` | `[]` | Required tool permissions |
| `dcc` | `str` | `"python"` | Target DCC application |
| `tags` | `List[str]` | `[]` | Classification tags |
| `scripts` | `List[str]` | `[]` | Discovered script file paths |
| `skill_path` | `str` | `""` | Absolute path to skill directory |
| `version` | `str` | `"1.0.0"` | Skill version |
| `depends` | `List[str]` | `[]` | Names of required skills |
| `metadata_files` | `List[str]` | `[]` | Files in metadata/ directory |
