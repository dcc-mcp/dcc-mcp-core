# Skills API

`dcc_mcp_core.SkillCatalog`, `dcc_mcp_core.SkillScanner`, `dcc_mcp_core.SkillWatcher`, `dcc_mcp_core.SkillMetadata`, `dcc_mcp_core.SkillSummary`, `dcc_mcp_core.ToolDeclaration`, `dcc_mcp_core.parse_skill_md`, `dcc_mcp_core.scan_and_load`

## SkillCatalog

Progressive skill discovery and loading. Manages skill lifecycle from discovery to active tool registration.

```python
from dcc_mcp_core import ActionRegistry, SkillCatalog

registry = ActionRegistry()
catalog = SkillCatalog(registry)
```

### Constructor

```python
SkillCatalog(registry: ActionRegistry) -> SkillCatalog
```

### Methods

| Method | Returns | Description |
|--------|---------|-------------|
| `discover(extra_paths=None, dcc_name=None)` | `int` | Discover skills; returns count of newly discovered |
| `load_skill(skill_name)` | `List[str]` | Load a skill; returns registered action names. Raises `ValueError` if not found |
| `unload_skill(skill_name)` | `int` | Unload a skill; returns count of removed actions. Raises `ValueError` if not loaded |
| `find_skills(query=None, tags=[], dcc=None)` | `List[SkillSummary]` | Search skills by query/tags/dcc (all filters AND-ed) |
| `list_skills(status=None)` | `List[SkillSummary]` | List all skills. `status`: `"loaded"`, `"discovered"`, `"error"`, or `None` for all |
| `get_skill_info(skill_name)` | `dict \| None` | Detailed skill info as dict, or `None` if not found |
| `is_loaded(skill_name)` | `bool` | Whether a skill is currently loaded |
| `loaded_count()` | `int` | Number of loaded skills |
| `__len__()` | `int` | Total skills in catalog |
| `__bool__()` | `bool` | False if catalog is empty |
| `__repr__()` | `str` | `SkillCatalog(total=N, loaded=N)` |

### Example

```python
import os
from dcc_mcp_core import ActionRegistry, SkillCatalog

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

registry = ActionRegistry()
catalog = SkillCatalog(registry)

# Discover skills
count = catalog.discover(extra_paths=["/extra/skills"], dcc_name="maya")

# List all discovered skills
for skill in catalog.list_skills():
    status = "loaded" if skill.loaded else "discovered"
    print(f"  [{status}] {skill.name} v{skill.version}: {skill.description}")

# Search
results = catalog.find_skills(query="geometry", tags=["create"])
for s in results:
    print(f"  {s.name}: {s.tool_count} tools → {s.tool_names}")

# Load a skill
actions = catalog.load_skill("maya-geometry")
print(f"Registered: {actions}")
# ['maya_geometry__create_sphere', 'maya_geometry__export_fbx']

# Inspect loaded skills
print(catalog.loaded_count(), len(catalog))

# Unload
n = catalog.unload_skill("maya-geometry")
print(f"Removed {n} actions")
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
