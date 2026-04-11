# Skills API

`dcc_mcp_core.SkillCatalog`, `dcc_mcp_core.SkillScanner`, `dcc_mcp_core.SkillWatcher`, `dcc_mcp_core.SkillMetadata`, `dcc_mcp_core.SkillSummary`, `dcc_mcp_core.ToolDeclaration`, `dcc_mcp_core.parse_skill_md`, `dcc_mcp_core.scan_and_load`

`dcc_mcp_core.skill` (pure-Python): `skill_entry`, `skill_success`, `skill_error`, `skill_warning`, `skill_exception`, `run_main`

## SkillCatalog

Progressive skill discovery and loading. Thread-safe (all state stored in DashMap/DashSet).

When a dispatcher is attached via `with_dispatcher()`, loading a skill also registers a subprocess-based handler for each action — enabling the Skills-First workflow where agents never need to register handlers manually.

```python
from dcc_mcp_core import SkillScanner, SkillCatalog

scanner = SkillScanner()
catalog = SkillCatalog(scanner)
```

### Constructor

```python
SkillCatalog(scanner: SkillScanner) -> SkillCatalog
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `scanner` | `SkillScanner` | Scanner instance used for discovery |

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `with_dispatcher(dispatcher)` | — | Attach an `ActionDispatcher`; enables auto-handler registration on `load_skill()` |
| `discover(extra_paths=None, dcc_name=None)` | `None` | Scan for skills and populate the catalog |
| `load_skill(skill_name)` | `bool` | Load a skill; returns `True` on success, `False` if already loaded or not found |
| `unload_skill(skill_name)` | `bool` | Unload a skill; returns `True` on success, `False` if not loaded |
| `find_skills(query=None, tags=None, dcc=None)` | `List[SkillSummary]` | Search by name/tags/dcc (all filters AND-ed) |
| `list_skills(status=None)` | `List[SkillSummary]` | List skills. `status`: `"loaded"` or `"unloaded"`, or `None` for all |
| `get_skill_info(skill_name)` | `SkillMetadata \| None` | Full metadata for a skill, or `None` if not found |
| `is_loaded(skill_name)` | `bool` | Whether a skill is currently loaded |
| `loaded_count()` | `int` | Number of loaded skills |
| `__repr__()` | `str` | String representation |

### Example

```python
import os
from dcc_mcp_core import SkillScanner, SkillCatalog, ActionRegistry, ActionDispatcher

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

scanner = SkillScanner()
catalog = SkillCatalog(scanner)

# Discover skills
catalog.discover(extra_paths=["/extra/skills"], dcc_name="maya")

# List all discovered skills
for skill in catalog.list_skills():
    status = "loaded" if skill.loaded else "unloaded"
    print(f"  [{status}] {skill.name} v{skill.version}: {skill.description}")

# Search
results = catalog.find_skills(query="geometry", tags=["create"])
for s in results:
    print(f"  {s.name}: {s.tool_count} tools → {s.tool_names}")

# With dispatcher — enables Skills-First auto-handler registration
registry = ActionRegistry()
dispatcher = ActionDispatcher(registry)
catalog.with_dispatcher(dispatcher)

# Load a skill (actions auto-registered if dispatcher is attached)
ok = catalog.load_skill("maya-geometry")
print(f"Loaded: {ok}")

# Get full metadata
meta = catalog.get_skill_info("maya-geometry")
if meta:
    print(meta.name, meta.tools)

# Inspect loaded skills
print(catalog.loaded_count())

# Unload
ok = catalog.unload_skill("maya-geometry")
print(f"Unloaded: {ok}")
```

---

## SkillSummary

Lightweight summary returned by `SkillCatalog.find_skills()` and `list_skills()`.

### Properties (read-only)

| Property | Type | Description |
|----------|------|-------------|
| `name` | `str` | Skill name |
| `description` | `str` | Short description |
| `tags` | `List[str]` | Skill tags |
| `dcc` | `str` | Target DCC (e.g. `"maya"`) |
| `version` | `str` | Skill version |
| `tool_count` | `int` | Number of declared tools |
| `tool_names` | `List[str]` | Names of declared tools |
| `loaded` | `bool` | Whether the skill is currently loaded |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `SkillSummary(name='...', loaded=True)` |

---

## ToolDeclaration

A single tool declaration within a skill, parsed from SKILL.md frontmatter `tools:` list.

```python
from dcc_mcp_core import ToolDeclaration

decl = ToolDeclaration(
    name="create_sphere",
    description="Create a polygon sphere",
    input_schema='{"type":"object","properties":{"radius":{"type":"number"}}}',
    read_only=False,
    destructive=False,
    idempotent=False,
    source_file="scripts/create_sphere.py",
)
```

### Constructor

```python
ToolDeclaration(
    name: str,
    description: str = "",
    input_schema: str | None = None,    # JSON Schema string
    output_schema: str | None = None,   # JSON Schema string
    read_only: bool = False,
    destructive: bool = False,
    idempotent: bool = False,
    source_file: str = "",
) -> ToolDeclaration
```

### Fields (read-write)

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | required | Tool name (unique within the skill) |
| `description` | `str` | `""` | Human-readable description |
| `read_only` | `bool` | `False` | True if this tool only reads data (no side effects) |
| `destructive` | `bool` | `False` | True if this tool may cause destructive changes |
| `idempotent` | `bool` | `False` | True if same args always produce the same result |
| `source_file` | `str` | `""` | Explicit path to the script file |

::: tip input_schema and output_schema
These are stored internally as JSON values, not strings. When constructing from Python, pass a JSON string and it will be parsed automatically.
:::

---

## SkillMetadata

Parsed from a skill's `SKILL.md` frontmatter. Supports Anthropic Skills, ClawHub/OpenClaw, and dcc-mcp-core extensions simultaneously.

```python
from dcc_mcp_core import SkillMetadata

meta = SkillMetadata(
    name="maya-geometry",
    description="Maya geometry tools",
    tools=[],        # List[str] — tool names
    dcc="maya",
    tags=["geometry"],
    search_hint="polygon modeling, sphere, bevel, mesh",
    scripts=[],      # List[str] — discovered script paths
    skill_path="/path/to/maya-geometry",
    version="1.0.0",
    depends=[],
    metadata_files=[],
)
```

### Constructor

```python
SkillMetadata(
    name: str,
    description: str = "",
    tools: List[str] | None = None,
    dcc: str = "python",
    tags: List[str] | None = None,
    search_hint: str = "",
    scripts: List[str] | None = None,
    skill_path: str = "",
    version: str = "1.0.0",
    depends: List[str] | None = None,
    metadata_files: List[str] | None = None,
) -> SkillMetadata
```

### Fields (read-write)

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Unique skill name |
| `description` | `str` | Short description |
| `search_hint` | `str` | Keyword hint for `search_skills` (SKILL.md `search-hint:` field; falls back to `description`) |
| `tools` | `List[str]` | Tool names from frontmatter |
| `dcc` | `str` | Target DCC application |
| `tags` | `List[str]` | Classification tags |
| `scripts` | `List[str]` | Discovered script file paths |
| `skill_path` | `str` | Absolute path to skill directory |
| `version` | `str` | Skill version |
| `depends` | `List[str]` | Dependency skill names |
| `metadata_files` | `List[str]` | Paths to `.md` files in `metadata/` |

---

## SkillScanner

Scanner for discovering skill packages in directories. Caches file modification times for efficient repeated scans.

```python
from dcc_mcp_core import SkillScanner

scanner = SkillScanner()
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `scan(extra_paths=None, dcc_name=None, force_refresh=False)` | `List[str]` | Scan paths for skill directories |
| `clear_cache()` | — | Clear the mtime cache and discovered list |

### Properties

| Property | Type | Description |
|----------|------|-------------|
| `discovered_skills` | `List[str]` | Previously discovered skill directory paths |

### Dunder Methods

| Method | Description |
|--------|-------------|
| `__repr__` | `SkillScanner(cached=N, discovered=N)` |

---

## SkillWatcher

Hot-reload watcher for skill directories. Monitors filesystem events and reloads skill metadata when `SKILL.md` files change.

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/path/to/skills")
skills = watcher.skills()
```

### Constructor

```python
SkillWatcher(debounce_ms: int = 300) -> SkillWatcher
```

`debounce_ms`: Milliseconds to wait before reloading after a change (multiple rapid events coalesced).

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `watch(path)` | — | Start watching `path` recursively. Raises `RuntimeError` if path does not exist |
| `unwatch(path)` | `bool` | Stop watching `path`. Returns `True` if was being watched |
| `skills()` | `List[SkillMetadata]` | Snapshot of all currently loaded skills |
| `skill_count()` | `int` | Number of skills currently loaded |
| `watched_paths()` | `List[str]` | List of currently watched directory paths |
| `reload()` | — | Manually trigger a full reload |
| `__repr__` | `str` | String representation |

---

## Functions

### parse_skill_md

```python
parse_skill_md(skill_dir: str) -> SkillMetadata | None
```

Parse a `SKILL.md` from a skill directory. Returns `None` if missing or invalid.

- Extracts YAML frontmatter between `---` delimiters
- Enumerates scripts in `scripts/` subdirectory
- Discovers `.md` files in `metadata/` subdirectory

### scan_skill_paths

```python
scan_skill_paths(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> List[str]
```

Convenience wrapper: creates a `SkillScanner` and returns discovered skill directory paths.

### scan_and_load

```python
scan_and_load(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[List[SkillMetadata], List[str]]
```

Full pipeline: scan directories, load all skills, and topologically sort by dependencies.

Returns `(ordered_skills, skipped_dirs)`. Raises `ValueError` on missing dependencies or cycles.

### scan_and_load_lenient

```python
scan_and_load_lenient(
    extra_paths: List[str] | None = None,
    dcc_name: str | None = None,
) -> tuple[List[SkillMetadata], List[str]]
```

Same as `scan_and_load` but silently skips skills with missing dependencies (warns via logging). Only cyclic dependencies raise `ValueError`.

Returns `(ordered_skills, skipped_dirs)`.

### resolve_dependencies

```python
resolve_dependencies(skills: List[SkillMetadata]) -> List[SkillMetadata]
```

Topologically sort skills so each skill appears after its dependencies. Raises `ValueError` on missing deps or cycles.

### validate_dependencies

```python
validate_dependencies(skills: List[SkillMetadata]) -> List[str]
```

Validate all declared dependencies exist. Returns a list of error messages (empty = no issues).

### expand_transitive_dependencies

```python
expand_transitive_dependencies(
    skills: List[SkillMetadata],
    skill_name: str,
) -> List[str]
```

Return names of all skills that `skill_name` transitively depends on. Raises `ValueError` on missing deps or cycles.

---

## Search Path Priority

1. `extra_paths` parameter (highest priority)
2. `DCC_MCP_{APP}_SKILL_PATHS` environment variable (per-app, e.g. `DCC_MCP_MAYA_SKILL_PATHS`)
3. `DCC_MCP_SKILL_PATHS` environment variable (global fallback)
4. Platform-specific skills directory (DCC-specific, via `get_skills_dir(dcc_name)`)
5. Platform-specific skills directory (global, via `get_skills_dir()`)

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_{APP}_SKILL_PATHS` | Per-app skill paths, e.g. `DCC_MCP_MAYA_SKILL_PATHS` (`;` on Windows, `:` on Unix) |
| `DCC_MCP_SKILL_PATHS` | Global fallback skill paths |

### create_skill_manager

```python
create_skill_manager(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer
```

**Recommended entry-point for the Skills-First workflow** (v0.12.12+).

Creates a fully wired `McpHttpServer` for a specific DCC application in one call. Automatically:
1. Creates `ActionRegistry` + `ActionDispatcher`
2. Creates `SkillCatalog` wired to the dispatcher
3. Discovers skills from `DCC_MCP_{APP}_SKILL_PATHS` and `DCC_MCP_SKILL_PATHS`
4. Returns a ready-to-start `McpHttpServer`

**Parameters:**

| Parameter | Type | Description |
|-----------|------|-------------|
| `app_name` | `str` | DCC name (e.g. `"maya"`, `"blender"`) — derives env var and MCP server name |
| `config` | `McpHttpConfig \| None` | HTTP server config; defaults to port 8765 |
| `extra_paths` | `list[str] \| None` | Extra skill dirs in addition to env vars |
| `dcc_name` | `str \| None` | Override DCC filter for scanning (defaults to `app_name`) |

**Returns:** `McpHttpServer` — call `.start()` to begin serving.

**Example:**

```python
import os
from dcc_mcp_core import create_skill_manager, McpHttpConfig

os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

server = create_skill_manager("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Serving at {handle.mcp_url()}")
```

### get_app_skill_paths_from_env

```python
get_app_skill_paths_from_env(app_name: str) -> list[str]
```

Return skill paths from the `DCC_MCP_{APP_NAME}_SKILL_PATHS` environment variable.

The lookup is case-insensitive; the actual env var key is upper-cased automatically (e.g. `DCC_MCP_MAYA_SKILL_PATHS` for `app_name="maya"`).

Returns `[]` if the env var is not set.

## Action Naming Convention

When `SkillCatalog.load_skill()` registers tools from a skill, action names follow the pattern:

```
{skill_name_underscored}__{tool_name}
```

Examples:
- skill `maya-geometry`, tool `create_sphere` → `maya_geometry__create_sphere`
- skill `blender-utils`, tool `render-scene` → `blender_utils__render_scene`

---

## Skill Script Helpers (pure-Python)

`dcc_mcp_core.skill` is a **pure-Python** sub-module — no compiled extension required.
Skill script authors can import helpers directly inside DCC environments that may not
have the full wheel installed.

```python
from dcc_mcp_core.skill import skill_entry, skill_success, skill_error
```

All helpers return a plain `dict` that is fully compatible with `ActionResultModel`.
When `dcc_mcp_core._core` is available, you can pass the dict to `validate_action_result()`
to obtain a typed `ActionResultModel` object.

---

### skill_success

```python
skill_success(
    message: str,
    *,
    prompt: str | None = None,
    **context,
) -> dict
```

Return a success result dict.

| Parameter | Type | Description |
|-----------|------|-------------|
| `message` | `str` | Human-readable summary of what was accomplished |
| `prompt` | `str \| None` | Optional hint for the agent's next action |
| `**context` | `Any` | Arbitrary key/value pairs attached to `context` |

```python
return skill_success(
    "Timeline set to frames 1–120",
    prompt="Check the timeline slider to verify.",
    start_frame=1,
    end_frame=120,
)
```

---

### skill_error

```python
skill_error(
    message: str,
    error: str,
    *,
    prompt: str | None = None,
    possible_solutions: list[str] | None = None,
    **context,
) -> dict
```

Return a failure result dict.

| Parameter | Type | Description |
|-----------|------|-------------|
| `message` | `str` | User-facing description of what went wrong |
| `error` | `str` | Technical error string (exception repr, code …) |
| `prompt` | `str \| None` | Recovery hint; defaults to a generic message |
| `possible_solutions` | `list[str] \| None` | Actionable suggestions in `context["possible_solutions"]` |

```python
return skill_error(
    "Maya is not available",
    "ImportError: No module named 'maya'",
    prompt="Ensure Maya is running before calling this skill.",
    possible_solutions=["Start Maya", "Check DCC_MCP_MAYA_SKILL_PATHS"],
)
```

---

### skill_warning

```python
skill_warning(
    message: str,
    *,
    warning: str = "",
    prompt: str | None = None,
    **context,
) -> dict
```

Return a success-but-with-warning result (`success=True`, `context["warning"]` set).

```python
return skill_warning(
    "Timeline set, end_frame clamped to scene length",
    warning="end_frame 9999 > scene length 240; clamped to 240",
    prompt="Verify the timeline slider.",
    actual_end=240,
)
```

---

### skill_exception

```python
skill_exception(
    exc: BaseException,
    *,
    message: str | None = None,
    prompt: str | None = None,
    include_traceback: bool = True,
    possible_solutions: list[str] | None = None,
    **context,
) -> dict
```

Return a failure result built from an exception. Captures `error_type` and optionally
the formatted traceback in `context`.

```python
try:
    do_work()
except Exception as exc:
    return skill_exception(
        exc,
        possible_solutions=["Check that the scene is open"],
    )
```

---

### @skill_entry

```python
@skill_entry
def my_tool(param: str = "default", **kwargs) -> dict:
    ...
```

Decorator that wraps a skill function with standard error handling.

- Catches `ImportError` (DCC module missing), `Exception`, and `BaseException`
- Converts each to a proper error dict automatically
- When run directly (`__name__ == "__main__"`), the JSON result is printed to stdout

**Full example** (replaces the manual try/except/`main()` boilerplate):

```python
from dcc_mcp_core.skill import skill_entry, skill_success

@skill_entry
def set_timeline(start_frame: float = 1.0, end_frame: float = 120.0, **kwargs):
    """Set the Maya playback timeline range."""
    import maya.cmds as cmds  # ImportError caught automatically if Maya not present

    min_frame = kwargs.get("min_frame", start_frame)
    max_frame = kwargs.get("max_frame", end_frame)

    cmds.playbackOptions(
        min=min_frame, max=max_frame,
        animationStartTime=start_frame, animationEndTime=end_frame,
    )
    return skill_success(
        f"Timeline set to {start_frame}–{end_frame}",
        prompt="Inspect the timeline slider to verify.",
        start_frame=start_frame,
        end_frame=end_frame,
    )

def main(**kwargs):
    """Entry point; delegates to set_timeline."""
    return set_timeline(**kwargs)

if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
```

---

### run_main

```python
run_main(main_fn: Callable[..., dict], argv: list[str] | None = None) -> None
```

Execute `main_fn` and print the JSON result to stdout. Calls `sys.exit(0)` on success,
`sys.exit(1)` on failure.

Intended for `if __name__ == "__main__"` blocks:

```python
if __name__ == "__main__":
    from dcc_mcp_core.skill import run_main
    run_main(main)
```

---

### Migration from DCC-specific helpers

If you previously used `dcc_mcp_maya`'s `maya_success` / `maya_error` / `maya_from_exception`,
the generic equivalents map directly:

| Old (DCC-specific) | New (generic) |
|--------------------|---------------|
| `maya_success(msg, prompt=..., **ctx)` | `skill_success(msg, prompt=..., **ctx)` |
| `maya_error(msg, error, prompt=..., **ctx)` | `skill_error(msg, error, prompt=..., **ctx)` |
| `maya_from_exception(exc_msg, ...)` | `skill_exception(exc, ...)` |

The dict structure is identical — both are compatible with `ActionResultModel`.

---

## Result Serialization — `serialize_result` / `deserialize_result`

Rust-backed serialization for `ActionResultModel`.  The format is switchable via
`SerializeFormat`: JSON today, MessagePack tomorrow — without changing calling code.

```python
from dcc_mcp_core import (
    serialize_result, deserialize_result, SerializeFormat, success_result
)
```

---

### SerializeFormat

```python
class SerializeFormat:
    Json: SerializeFormat     # UTF-8 JSON text (default)
    MsgPack: SerializeFormat  # binary MessagePack via rmp-serde
```

---

### serialize_result

```python
serialize_result(
    result: ActionResultModel,
    format: SerializeFormat = SerializeFormat.Json,
) -> str | bytes
```

Serialize an `ActionResultModel`.

| `format` | Return type | Description |
|----------|-------------|-------------|
| `SerializeFormat.Json` | `str` | UTF-8 JSON string |
| `SerializeFormat.MsgPack` | `bytes` | Binary MessagePack |

```python
arm = success_result("Timeline updated", start_frame=1, end_frame=120)

# JSON (default)
json_str = serialize_result(arm)
assert isinstance(json_str, str)

# MessagePack
msgpack_bytes = serialize_result(arm, SerializeFormat.MsgPack)
assert isinstance(msgpack_bytes, bytes)
```

---

### deserialize_result

```python
deserialize_result(
    data: str | bytes,
    format: SerializeFormat = SerializeFormat.Json,
) -> ActionResultModel
```

Deserialize a `str` (JSON) or `bytes` (MsgPack) back into an `ActionResultModel`.
The *format* must match what was used during serialization.

```python
original = success_result("done", frame_count=240)
roundtrip = deserialize_result(serialize_result(original))
assert roundtrip.success
assert roundtrip.message == "done"
assert roundtrip.context["frame_count"] == 240
```

---

### How `run_main` uses serialization

`run_main()` automatically uses `serialize_result` when `_core` is available,
falling back to `json.dumps` in pure-Python environments:

```
result dict
    ↓ validate_action_result()  (type-safe validation)
ActionResultModel
    ↓ serialize_result(arm, SerializeFormat.Json)   (Rust JSON writer)
JSON string → stdout
```

To switch to MessagePack in a future release, only `_serialize_result()` in
`skill.py` needs updating — the `serialize_result` / `deserialize_result` API
remains stable.
