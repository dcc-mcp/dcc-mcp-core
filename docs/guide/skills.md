# Skills System

The Skills system allows you to register any script (Python, MEL, MaxScript, BAT, Shell, etc.) as an MCP-discoverable tool with **zero Python code**. It directly reuses the [OpenClaw Skills](https://docs.openclaw.ai/tools) / Anthropic Skills ecosystem format.

## Quick Start

### 1. Create a Skill Directory

```
maya-geometry/
├── SKILL.md
└── scripts/
    ├── create_sphere.py
    ├── batch_rename.mel
    └── export_fbx.bat
```

### 2. Write the SKILL.md

```yaml
---
name: maya-geometry
description: "Maya geometry creation and modification tools"
version: "1.0.0"
dcc: maya
tags: ["geometry", "create"]
search-hint: "polygon modeling, sphere, bevel, extrude, mesh editing"
tools:
  - name: create_sphere
    description: "Create a polygon sphere with the given radius"
    source_file: scripts/create_sphere.py
    read_only: false
  - name: export_fbx
    description: "Export selected objects to FBX"
    source_file: scripts/export_fbx.bat
---
# Maya Geometry Skill

Use these tools to create and modify geometry in Maya.
```

The `search-hint` field provides comma-separated keywords for efficient skill discovery
via `search_skills` without loading full tool schemas.

### 3. Set Environment Variable

```bash
# Linux/macOS
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# Windows
set DCC_MCP_SKILL_PATHS=C:\path\to\my-skills

# Multiple paths (use platform path separator)
export DCC_MCP_SKILL_PATHS="/path/skills1:/path/skills2"
```

### 4. Discover and Load

Use `SkillCatalog` (recommended) for full progressive loading, or low-level scan functions for one-shot use:

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry

registry = ToolRegistry()
catalog = SkillCatalog(registry)

# Discover all skills from DCC_MCP_SKILL_PATHS
discovered = catalog.discover(dcc_name="maya")
print(f"Discovered {discovered} skills")

# List available skills
for skill in catalog.list_skills():
    print(f"  {skill.name} v{skill.version}: {skill.description} (loaded={skill.loaded})")

# Load a skill — returns the registered action names
actions = catalog.load_skill("maya-geometry")
print(actions)
```

## Skill Catalog (Recommended API)

`SkillCatalog` manages the full lifecycle: discovery → progressive loading → unloading.

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry

registry = ToolRegistry()
catalog = SkillCatalog(registry)

# Discovery
catalog.discover(extra_paths=["/my/skills"], dcc_name="maya")

# Search
results = catalog.find_skills(query="geometry", tags=["create"], dcc="maya")
for s in results:
    print(f"{s.name}: {s.tool_count} tools {s.tool_names}")

# Load/unload
actions = catalog.load_skill("maya-geometry")  # returns List[str]
catalog.is_loaded("maya-geometry")        # True
removed = catalog.unload_skill("maya-geometry")

# Status inspection
catalog.loaded_count()      # int
len(catalog)                # total skills in catalog
catalog.list_skills()       # all skills (SkillSummary list)
catalog.list_skills("loaded")      # only loaded
catalog.list_skills("unloaded")    # only unloaded

# Detail
info = catalog.get_skill_info("maya-geometry")  # dict with full details or None
```

### SkillSummary Fields

`find_skills()`, `list_skills()`, and `search_skills` (MCP tool) return `SkillSummary` objects:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Skill name |
| `description` | `str` | Short description |
| `search_hint` | `str` | Keyword hint for discovery (from `search-hint:` in SKILL.md; falls back to `description`) |
| `tags` | `List[str]` | Skill tags |
| `dcc` | `str` | Target DCC (e.g. `"maya"`) |
| `version` | `str` | Skill version |
| `tool_count` | `int` | Number of declared tools |
| `tool_names` | `List[str]` | Names of declared tools |
| `loaded` | `bool` | Whether the skill is currently loaded |

## ToolDeclaration

A `ToolDeclaration` describes a single tool within a skill. Declared in the `tools:` list in SKILL.md frontmatter:

```yaml
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    input_schema: '{"type":"object","properties":{"radius":{"type":"number"}}}'
    read_only: false
    destructive: false
    idempotent: false
    defer-loading: true
    source_file: scripts/create_sphere.py
```

```python
from dcc_mcp_core import ToolDeclaration

decl = ToolDeclaration(
    name="create_sphere",
    description="Create a polygon sphere",
    input_schema='{"type":"object","properties":{"radius":{"type":"number"}}}',
    read_only=False,
    destructive=False,
    idempotent=False,
    defer_loading=True,
    source_file="scripts/create_sphere.py",
)
```

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | required | Tool name (unique within the skill) |
| `description` | `str` | `""` | Human-readable description |
| `input_schema` | `str` (JSON) | `None` | JSON Schema for input parameters |
| `output_schema` | `str` (JSON) | `None` | JSON Schema for output |
| `read_only` | `bool` | `False` | Whether this tool only reads data |
| `destructive` | `bool` | `False` | Whether this tool may cause destructive changes |
| `idempotent` | `bool` | `False` | Whether calling with same args always produces same result |
| `defer_loading` | `bool` | `False` | Accepts `defer-loading` / `defer_loading` in SKILL.md and marks the declaration as discovery-oriented |
| `source_file` | `str` | `""` | Explicit path to the script (relative to skill dir) |

Unloaded skill stubs returned by `tools/list` also expose `annotations.deferredHint = true` as an explicit progressive-loading signal. Once you call `load_skill(...)`, the real tools replace the stub and return `deferredHint = false`.

## Script Lookup Priority

When loading a skill, the catalog resolves which script backs each tool declaration:

1. `ToolDeclaration.source_file` — explicit path wins
2. A script in `scripts/` whose stem matches the tool name
3. If the skill has only one script, it backs all tools
4. No handler registered (tool visible in registry but not executable)

## Low-Level Skill Functions

For simple one-shot scanning without progressive loading:

```python
import os
from dcc_mcp_core import (
    SkillScanner,
    SkillWatcher,
    SkillMetadata,
    parse_skill_md,
    scan_skill_paths,
    scan_and_load,
    scan_and_load_lenient,
)

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

# One-shot scan + load + dependency sort → returns (skills, skipped_dirs)
skills, skipped = scan_and_load(extra_paths=["/my/skills"], dcc_name="maya")
skills_lenient, skipped = scan_and_load_lenient(dcc_name="maya")  # skip errors

# Scan directories for SKILL.md files
scanner = SkillScanner()
skill_dirs = scanner.scan(extra_paths=["/my/skills"], dcc_name="maya")

# Parse a single skill directory
metadata = parse_skill_md("/path/to/maya-geometry")

# Get raw list of skill directory paths
paths = scan_skill_paths(extra_paths=["/my/skills"], dcc_name="maya")
```

## Live Reload with SkillWatcher

`SkillWatcher` monitors the filesystem and automatically reloads skills when `SKILL.md` files change:

```python
from dcc_mcp_core import SkillWatcher

watcher = SkillWatcher(debounce_ms=300)
watcher.watch("/path/to/skills")

# Get current skills (snapshot)
skills = watcher.skills()          # List[SkillMetadata]
count = watcher.skill_count()      # int

# Manual reload
watcher.reload()

# Stop watching
watcher.unwatch("/path/to/skills")

# Inspect watched paths
paths = watcher.watched_paths()    # List[str]
```

## Dependency Resolution

Skills can declare dependencies on other skills using the `depends:` field in SKILL.md:

```yaml
---
name: maya-animation
depends: ["maya-geometry"]
---
```

```python
from dcc_mcp_core import (
    resolve_dependencies,
    validate_dependencies,
    expand_transitive_dependencies,
)

skills, _ = scan_and_load()

# Topologically sorted (each skill after its dependencies)
ordered = resolve_dependencies(skills)

# Validate — returns list of error messages
errors = validate_dependencies(skills)

# All transitive dependencies for a specific skill
deps = expand_transitive_dependencies(skills, "maya-animation")
# ["maya-geometry"]
```

## SkillMetadata Fields

Parsed from SKILL.md frontmatter. Supports Anthropic Skills, ClawHub/OpenClaw, and dcc-mcp-core extensions simultaneously.

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Unique skill name |
| `description` | `str` | Short description |
| `search_hint` | `str` | Keyword hint for `search_skills` (SKILL.md `search-hint:` field; falls back to `description`) |
| `tools` | `List[str]` | Tool names listed in frontmatter |
| `dcc` | `str` | Target DCC application (default: `"python"`) |
| `tags` | `List[str]` | Classification tags |
| `scripts` | `List[str]` | Discovered script file paths |
| `skill_path` | `str` | Absolute path to the skill directory |
| `version` | `str` | Skill version (default: `"1.0.0"`) |
| `depends` | `List[str]` | Skill dependency names |
| `metadata_files` | `List[str]` | Paths to `.md` files in `metadata/` |

## Environment Variables

| Variable | Description |
|----------|-------------|
| `DCC_MCP_{APP}_SKILL_PATHS` | Per-app skill paths, e.g. `DCC_MCP_MAYA_SKILL_PATHS` (`;` on Windows, `:` on Unix) |
| `DCC_MCP_SKILL_PATHS` | Global fallback skill paths (used when per-app var is not set) |

::: tip Per-app paths take priority
`DCC_MCP_MAYA_SKILL_PATHS` is checked first for `app_name="maya"`. `DCC_MCP_SKILL_PATHS` is the global fallback.
:::

## one-call Skills-First Setup: `create_skill_server`

For the fastest possible setup, use `create_skill_server` (v0.12.12+). It wires together `ToolRegistry`, `ToolDispatcher`, `SkillCatalog`, and `McpHttpServer` in one call:

```python
import os
from dcc_mcp_core import create_skill_server, McpHttpConfig

# Set per-app skill paths
os.environ["DCC_MCP_MAYA_SKILL_PATHS"] = "/studio/maya-skills"

# One call: discover skills + start MCP HTTP server
server = create_skill_server("maya", McpHttpConfig(port=8765))
handle = server.start()
print(f"Maya MCP server at {handle.mcp_url()}")
# AI clients connect to http://127.0.0.1:8765/mcp
```

`create_skill_server` automatically:
1. Creates an `ToolRegistry` and `ToolDispatcher`
2. Creates a `SkillCatalog` wired to the dispatcher
3. Discovers skills from `DCC_MCP_MAYA_SKILL_PATHS` and `DCC_MCP_SKILL_PATHS`
4. Returns a ready-to-start `McpHttpServer`

```python
def create_skill_server(
    app_name: str,
    config: McpHttpConfig | None = None,
    extra_paths: list[str] | None = None,
    dcc_name: str | None = None,
) -> McpHttpServer: ...
```

| Parameter | Type | Description |
|-----------|------|-------------|
| `app_name` | `str` | DCC application name (`"maya"`, `"blender"`, etc.) — derives env var and server name |
| `config` | `McpHttpConfig \| None` | HTTP config; defaults to port 8765 |
| `extra_paths` | `list[str] \| None` | Extra skill dirs to scan in addition to env vars |
| `dcc_name` | `str \| None` | Override DCC filter for skill scanning (defaults to `app_name`) |

## Supported Script Types

| Extension | Type | Execution |
|-----------|------|-----------|
| `.py` | Python | `python` interpreter |
| `.sh`, `.bash` | Shell | `bash` |
| `.bat`, `.cmd` | Batch | `cmd /C` |
| `.mel` | MEL (Maya) | `python` wrapper |
| `.ms` | MaxScript (3ds Max) | `python` wrapper |
| `.lua`, `.hscript` | Lua / Houdini | `python` wrapper |

::: tip Skills-First Architecture
`create_skill_server` is the recommended entry-point since v0.12.12. It combines `SkillCatalog` automatic script execution with MCP HTTP serving — agents can call tools via `tools/call` with zero manual handler registration.
:::

::: warning script execution
All scripts run as subprocesses. Input parameters are passed via stdin as JSON. The script should write a JSON result to stdout and exit with code 0 on success.
:::

## On-Demand Skill Discovery (MCP HTTP)

When using the MCP HTTP server (`McpHttpServer` or `create_skill_server`), `tools/list` returns a **three-tier** response:

### Three-Tier `tools/list` Response

1. **6 core discovery tools** (always present):
   - `find_skills` — Search for skills by query, tags, DCC type
   - `list_skills` — List all skills with optional status filter
   - `get_skill_info` — Get full metadata for a specific skill
   - `load_skill` — Load a skill, registering its tools in ToolRegistry
   - `unload_skill` — Unload a skill, removing its tools
   - `search_skills` — Keyword search across name, description, search_hint, and tool_names

2. **Loaded skill tools** — Full `input_schema` from the `ToolRegistry` for all currently loaded skills

3. **Unloaded skill stubs** — `__skill__<name>` entries with a one-line description only (no full schema)

### Workflow

```
1. AI calls tools/list → sees core tools + loaded tools + __skill__ stubs
2. AI calls search_skills(query="geometry") → finds matching skills
3. AI calls load_skill(skill_name="maya-geometry") → tools registered
4. AI calls tools/list again → maya-geometry tools now have full schemas
5. AI calls maya_geometry__create_sphere → skill script executes
```

### Skill Stub Behavior

Calling an unloaded skill stub (`__skill__<name>`) returns an error with a hint:

```json
{
  "error": "Skill 'maya-geometry' is not loaded. Call load_skill(skill_name=\"maya-geometry\") to register its tools."
}
```

### `search_skills` MCP Tool

```json
{
  "name": "search_skills",
  "description": "Search for skills matching a keyword query",
  "inputSchema": {
    "type": "object",
    "properties": {
      "query": {"type": "string", "description": "Search keyword"},
      "dcc": {"type": "string", "description": "Filter by DCC type"}
    },
    "required": ["query"]
  }
}
```

Searches across: `name`, `description`, `search_hint`, and `tool_names`. The `search_hint` field (from SKILL.md `search-hint:`) improves keyword matching without loading full schemas.

`create_skill_server()` only calls `discover()` at startup — skills are **not** automatically loaded. This keeps the initial tool list small and lets agents load only what they need.
