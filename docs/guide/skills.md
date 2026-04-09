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
from dcc_mcp_core import ActionRegistry, SkillCatalog

# Create registry and catalog
registry = ActionRegistry()
catalog = SkillCatalog(registry)

# Discover all skills from DCC_MCP_SKILL_PATHS
count = catalog.discover(dcc_name="maya")
print(f"Discovered {count} skills")

# List available skills
for skill in catalog.list_skills():
    print(f"  {skill.name} v{skill.version}: {skill.description} (loaded={skill.loaded})")

# Load a skill — registers its tools in ActionRegistry
actions = catalog.load_skill("maya-geometry")
print(f"Registered actions: {actions}")
# ['maya_geometry__create_sphere', 'maya_geometry__export_fbx']
```

## Skill Catalog (Recommended API)

`SkillCatalog` manages the full lifecycle: discovery → progressive loading → unloading.

```python
from dcc_mcp_core import ActionRegistry, ActionDispatcher, SkillCatalog

registry = ActionRegistry()
dispatcher = ActionDispatcher(registry)
catalog = SkillCatalog(registry)

# Discovery
count = catalog.discover(extra_paths=["/my/skills"], dcc_name="maya")

# Search
results = catalog.find_skills(query="geometry", tags=["create"], dcc="maya")
for s in results:
    print(f"{s.name}: {s.tool_count} tools {s.tool_names}")

# Load/unload
actions = catalog.load_skill("maya-geometry")  # returns List[str] of action names
catalog.is_loaded("maya-geometry")             # True
n_removed = catalog.unload_skill("maya-geometry")

# Status inspection
catalog.loaded_count()      # int
len(catalog)                # total skills in catalog
catalog.list_skills()       # all skills (SkillSummary list)
catalog.list_skills("loaded")      # only loaded
catalog.list_skills("discovered")  # only unloaded

# Detail
info = catalog.get_skill_info("maya-geometry")  # dict with full details or None
```

### SkillSummary Fields

`find_skills()` and `list_skills()` return `SkillSummary` objects:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Skill name |
| `description` | `str` | Short description |
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
| `source_file` | `str` | `""` | Explicit path to the script (relative to skill dir) |

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
| `DCC_MCP_SKILL_PATHS` | Skill search paths (`;` on Windows, `:` on Unix) |

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
Starting with v0.12.10, `SkillCatalog` supports automatic script execution handlers. When a dispatcher is attached, loading a skill also registers subprocess-based handlers — agents can call tools via `tools/call` without any manual handler registration.
:::

::: warning script execution
All scripts run as subprocesses. Input parameters are passed via stdin as JSON. The script should write a JSON result to stdout and exit with code 0 on success.
:::
