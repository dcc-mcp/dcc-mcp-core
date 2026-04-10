# Models

## ActionResultModel

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
| `with_error(error)` | `ActionResultModel` | Create copy with error info (sets `success=False`) |
| `with_context(**kwargs)` | `ActionResultModel` | Create copy with updated context |
| `to_dict()` | `Dict[str, Any]` | Convert to dictionary |
| `__eq__(other)` | `bool` | Equality comparison |
| `__str__()` | `str` | Human-readable string |
| `__repr__()` | `str` | Unambiguous representation |

### Factory Functions

```python
from dcc_mcp_core import success_result, error_result, from_exception, validate_action_result

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

# Validate/normalize any value to ActionResultModel
validate_action_result(result)                          # pass-through
validate_action_result({"success": True, "message": "OK"})  # dict → ARM
validate_action_result("hello")                         # wrap as success
```

### Factory Function Signatures

| Function | Signature | Description |
|----------|-----------|-------------|
| `success_result` | `(message, prompt=None, **context) -> ActionResultModel` | Create a successful result |
| `error_result` | `(message, error, prompt=None, possible_solutions=None, **context) -> ActionResultModel` | Create a failed result |
| `from_exception` | `(error_message, message=None, prompt=None, include_traceback=True, possible_solutions=None, **context) -> ActionResultModel` | Wrap an exception as a result |
| `validate_action_result` | `(result: Any) -> ActionResultModel` | Normalize dict/str/None/ARM → ActionResultModel |

## SkillMetadata

Metadata parsed from SKILL.md frontmatter. All fields are readable and writable.

### Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | — | Unique identifier |
| `description` | `str` | `""` | Human-readable description |
| `tools` | `List[str]` | `[]` | Required tool permissions |
| `dcc` | `str` | `"python"` | Target DCC application |
| `tags` | `List[str]` | `[]` | Classification tags |
| `scripts` | `List[str]` | `[]` | Discovered script file paths |
| `skill_path` | `str` | `""` | Absolute path to skill directory |
| `version` | `str` | `"1.0.0"` | Skill version |
| `depends` | `List[str]` | `[]` | Names of required skills |
| `metadata_files` | `List[str]` | `[]` | Files in metadata/ directory |
