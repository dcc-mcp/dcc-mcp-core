# Utilities API

`dcc_mcp_core.utils` — filesystem, constants, type wrappers, logging.

## Type Wrappers (RPyC)

Type-safe wrappers for transmitting Python values over RPyC connections.

```python
from dcc_mcp_core import wrap_value, unwrap_value, unwrap_parameters
from dcc_mcp_core import BooleanWrapper, IntWrapper, FloatWrapper, StringWrapper

# Wrap native values
wrapped = wrap_value(True)         # BooleanWrapper(True)
wrapped = wrap_value(42)           # IntWrapper(42)
wrapped = wrap_value(3.14)         # FloatWrapper(3.14)
wrapped = wrap_value("hello")     # StringWrapper("hello")

# Unwrap back to native
original = unwrap_value(wrapped)   # True, 42, 3.14, or "hello"

# Unwrap all values in a dict
params = {"visible": BooleanWrapper(True), "count": IntWrapper(5)}
native = unwrap_parameters(params)  # {"visible": True, "count": 5}
```

### Wrapper Classes

| Class | Python Type | Special Methods |
|-------|-------------|-----------------|
| `BooleanWrapper(value)` | `bool` | `__bool__`, `__eq__`, `__hash__` |
| `IntWrapper(value)` | `int` (`i64`) | `__int__`, `__index__`, `__eq__`, `__hash__` |
| `FloatWrapper(value)` | `float` (`f64`) | `__float__`, `__eq__` (relative tolerance), **not hashable** |
| `StringWrapper(value)` | `str` | `__str__`, `__eq__`, `__hash__` |

::: warning
`FloatWrapper` is intentionally not hashable (`__hash__` raises `TypeError`) because `f64` does not implement `Eq`/`Hash` (NaN ≠ NaN). Do not use it as a dict key or in sets.
:::

## Filesystem

Platform-specific directory resolution using the `dirs` crate.

```python
from dcc_mcp_core import (
    get_platform_dir, get_config_dir, get_data_dir, get_log_dir,
    get_actions_dir, get_skills_dir, get_skill_paths_from_env,
)

config = get_config_dir()           # ~/.config/dcc-mcp (Linux)
data = get_data_dir()               # ~/.local/share/dcc-mcp (Linux)
log = get_log_dir()                 # ~/.local/share/dcc-mcp/log (Linux)
actions = get_actions_dir("maya")   # {data}/actions/maya/
skills = get_skills_dir("maya")     # {data}/skills/maya/
skills = get_skills_dir()           # {data}/skills/ (global)

# Generic platform dir (config, data, cache, log, documents)
dir = get_platform_dir("cache")

# From environment variable
paths = get_skill_paths_from_env()  # splits DCC_MCP_SKILL_PATHS
```

::: tip
All `get_*_dir()` functions create the directory if it doesn't exist (except `get_skills_dir` which only returns the path).
:::

## Constants

Available as module-level attributes:

| Constant | Value | Description |
|----------|-------|-------------|
| `APP_NAME` | `"dcc-mcp"` | App name for platform dirs |
| `APP_AUTHOR` | `"dcc-mcp"` | App author |
| `DEFAULT_DCC` | `"python"` | Default DCC name |
| `DEFAULT_LOG_LEVEL` | `"DEBUG"` | Default log level |
| `ENV_LOG_LEVEL` | `"MCP_LOG_LEVEL"` | Env var for log level |
| `ENV_SKILL_PATHS` | `"DCC_MCP_SKILL_PATHS"` | Env var for skill paths |
| `SKILL_METADATA_FILE` | `"SKILL.md"` | Skill metadata filename |
| `SKILL_SCRIPTS_DIR` | `"scripts"` | Scripts subdirectory |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_SKILL_PATHS` | Skill search paths (`;` on Windows, `:` on Unix) |
| `MCP_LOG_LEVEL` | Log level override (DEBUG, INFO, WARN, ERROR) |
