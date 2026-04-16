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
    get_tools_dir, get_skills_dir, get_skill_paths_from_env,
    get_app_skill_paths_from_env,
)

config = get_config_dir()           # ~/.config/dcc-mcp (Linux)
data = get_data_dir()               # ~/.local/share/dcc-mcp (Linux)
log = get_log_dir()                 # ~/.local/share/dcc-mcp/log (Linux)
tools = get_tools_dir("maya")   # {data}/actions/maya/
skills = get_skills_dir("maya")     # {data}/skills/maya/
skills = get_skills_dir()           # {data}/skills/ (global)

# Generic platform dir (config, data, cache, log, documents)
dir = get_platform_dir("cache")

# From global environment variable
paths = get_skill_paths_from_env()  # splits DCC_MCP_SKILL_PATHS

# From per-app environment variable
paths = get_app_skill_paths_from_env("maya")     # reads DCC_MCP_MAYA_SKILL_PATHS
paths = get_app_skill_paths_from_env("blender")  # reads DCC_MCP_BLENDER_SKILL_PATHS
```

::: tip
All `get_*_dir()` functions create the directory if it doesn't exist (except `get_skills_dir` which only returns the path).
:::

### `get_app_skill_paths_from_env`

```python
def get_app_skill_paths_from_env(app_name: str) -> list[str]: ...
```

Return skill paths from the `DCC_MCP_{APP_NAME}_SKILL_PATHS` environment variable.

| Parameter | Type | Description |
|-----------|------|-------------|
| `app_name` | `str` | DCC application name, e.g. `"maya"`, `"blender"`. Case-insensitive — automatically uppercased to form the env var key. |

**Returns**: `list[str]` — directory paths from the env var, or `[]` if not set.

```python
import os
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills;/home/user/skills"

from dcc_mcp_core import get_app_skill_paths_from_env

paths = get_app_skill_paths_from_env("maya")
# ["/studio/maya-skills", "/home/user/skills"]

# Case-insensitive: "Maya", "MAYA", "maya" all read DCC_MCP_MAYA_SKILL_PATHS
paths = get_app_skill_paths_from_env("Maya")  # same result
```

## Skill Functions

Top-level functions for scanning, loading, and resolving skill dependencies.

### `parse_skill_md()`

```python
from dcc_mcp_core import parse_skill_md

metadata = parse_skill_md("/path/to/skills/maya-geometry")
if metadata:
    print(metadata.name, metadata.version)  # "maya-geometry", "1.0.0"
```

### `scan_skill_paths()`

Scan configured directories for skill packages:

```python
from dcc_mcp_core import scan_skill_paths

# Scan all configured paths (env vars + platform dirs)
paths = scan_skill_paths()

# Scope to a specific DCC
paths = scan_skill_paths(dcc_name="maya")

# Add extra directories
paths = scan_skill_paths(extra_paths=["/my/extra/skills"], dcc_name="blender")
```

### `scan_and_load()`

Full pipeline: scan directories, parse `SKILL.md` files, and topologically sort by dependencies. Raises `ValueError` on missing dependencies or cycles.

```python
from dcc_mcp_core import scan_and_load

skills, skipped = scan_and_load(dcc_name="maya")

for skill in skills:
    print(f"{skill.name} v{skill.version}")

print(f"Skipped {len(skipped)} directories")
```

### `scan_and_load_lenient()`

Like `scan_and_load()` but silently skips skills with missing dependencies (only fails on cycles):

```python
from dcc_mcp_core import scan_and_load_lenient

skills, skipped = scan_and_load_lenient(dcc_name="maya")
# skills with unresolvable deps are in skipped, not raised as errors
```

### `resolve_dependencies()`

Topologically sort a list of `SkillMetadata` objects by their `depends` field:

```python
from dcc_mcp_core import resolve_dependencies

ordered = resolve_dependencies(skills)  # skills sorted so deps come first
```

Raises `ValueError` if a dependency is missing or a cycle is detected.

### `validate_dependencies()`

Validate that all declared dependencies exist in the provided skill list:

```python
from dcc_mcp_core import validate_dependencies

errors = validate_dependencies(skills)
for err in errors:
    print(f"Missing dep: {err}")
```

Returns a list of error messages. Empty list means all deps are satisfied.

### `expand_transitive_dependencies()`

Get all transitive dependencies of a skill (recursively):

```python
from dcc_mcp_core import expand_transitive_dependencies

all_deps = expand_transitive_dependencies(skills, "maya-complex-tool")
print(all_deps)  # ["base-utils", "maya-core", "maya-geometry"]
```

Raises `ValueError` on missing deps or cycles.

## Constants

Available as module-level attributes:

| Constant | Value | Description |
|----------|-------|-------------|
| `APP_NAME` | `"dcc-mcp"` | App name for platform dirs |
| `APP_AUTHOR` | `"dcc-mcp"` | App author |
| `DEFAULT_DCC` | `"python"` | Default DCC name |
| `DEFAULT_LOG_LEVEL` | `"DEBUG"` | Default log level |
| `ENV_LOG_LEVEL` | `"MCP_LOG_LEVEL"` | Env var for log level |
| `ENV_SKILL_PATHS` | `"DCC_MCP_SKILL_PATHS"` | Env var for global skill paths |
| `ENV_APP_SKILL_PATHS` | `"DCC_MCP_{APP}_SKILL_PATHS"` | Template for per-app skill paths env var |
| `SKILL_METADATA_FILE` | `"SKILL.md"` | Skill metadata filename |
| `SKILL_SCRIPTS_DIR` | `"scripts"` | Scripts subdirectory |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_SKILL_PATHS` | Global skill search paths (`;` on Windows, `:` on Unix) |
| `DCC_MCP_{APP}_SKILL_PATHS` | Per-app skill paths, e.g. `DCC_MCP_MAYA_SKILL_PATHS` for Maya |
| `MCP_LOG_LEVEL` | Log level override (DEBUG, INFO, WARN, ERROR) |

::: tip Search Path Priority
When `create_skill_server("maya")` is called, skill directories are resolved in this order:
1. `extra_paths` argument (highest priority)
2. Per-app env var: `DCC_MCP_MAYA_SKILL_PATHS`
3. Global env var: `DCC_MCP_SKILL_PATHS`
4. Platform data dir: `~/.local/share/dcc-mcp/skills/maya/`
:::
