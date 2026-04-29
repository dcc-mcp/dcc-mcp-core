# Skills System

The Skills system allows you to register any script (Python, MEL, MaxScript, BAT, Shell, etc.) as an MCP-discoverable tool with **zero Python code**. It follows the [agentskills.io V1.0](https://agentskills.io/specification) specification for SKILL.md format, with DCC-specific extensions (`dcc`, `search-hint`, `tools`, `groups`, `depends`, `external_deps`).

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

# Load a skill — returns the registered tool names
tool_names = catalog.load_skill("maya-geometry")
print(tool_names)
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
results = catalog.search_skills(query="geometry", tags=["create"], dcc="maya")
for s in results:
    print(f"{s.name}: {s.tool_count} tools {s.tool_names}")

# Load/unload
actions = catalog.load_skill("maya-geometry")  # returns List[str] — tool names
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

`search_skills()` and `list_skills()` return `SkillSummary` objects:

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

### `next-tools` — Follow-Up Tool Hints (dcc-mcp-core extension)

The `next-tools` field guides AI agents to appropriate follow-up actions after a tool
executes. This is a dcc-mcp-core extension not present in the agentskills.io specification.

```yaml
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    source_file: scripts/create_sphere.py
    next-tools:
      on-success: [maya_geometry__bevel_edges]      # suggest after success
      on-failure: [dcc_diagnostics__screenshot]      # debug on failure
```

| Key | Type | Description |
|-----|------|-------------|
| `on-success` | `List[str]` | Suggested tools after successful execution |
| `on-failure` | `List[str]` | Debugging/recovery tools on failure |

Both accept lists of fully-qualified tool names in `{skill_name}__{tool_name}` format. SEP-986 dot-namespacing (`skill.tool_name`) is also supported — see [Naming Rules](/guide/naming) for validation.

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
    scan_and_load_strict,
)

os.environ["DCC_MCP_SKILL_PATHS"] = "/path/to/skills"

# One-shot scan + load + dependency sort → returns (skills, skipped_dirs)
skills, skipped = scan_and_load(extra_paths=["/my/skills"], dcc_name="maya")
skills_lenient, skipped = scan_and_load_lenient(dcc_name="maya")  # skip errors

# Strict variant (issue maya#138): raises ValueError when any directory
# was silently skipped, so embedders can fail start-up loudly instead of
# discovering the missing tools at run-time. The exception message lists
# every offending directory and points at scan_and_load_lenient as the
# opt-out for installations that genuinely want the silent-skip default.
try:
    skills, _ = scan_and_load_strict(dcc_name="maya")
except ValueError as exc:
    # Wire this into your DCC plugin's start-up error reporting.
    raise SystemExit(f"Refusing to start: {exc}") from exc
```

::: tip Choosing the right entry point
- `scan_and_load` — default; logs a `warn`-level summary for every skipped
  directory (issue maya#138) and a per-failure `warn` from
  `parse_skill_md`. Good for production where missing skills should be
  visible in logs but not block start-up.
- `scan_and_load_lenient` — same return shape but tolerates resolver
  errors (missing dependency, dependency cycle).
- `scan_and_load_strict` — fails fast when the scanner skipped any
  directory. Use in CI / packaged adapter releases where a malformed
  `SKILL.md` should be a blocking error rather than a silent omission.
:::

```python

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

## Layered Skill Architecture

### Why layers matter

As a DCC adapter's skill set grows, AI agents can encounter routing ambiguity: two
skills share overlapping keywords and the agent picks the wrong one. The layered
architecture solves this by making each skill's scope and counter-examples explicit
in its `description`, and by tagging every skill with a `dcc-mcp.layer` metadata key
that partitions discovery.

### The three layers

| Layer | Role | `dcc-mcp.layer` value |
|-------|------|----------------------|
| **infrastructure** | Low-level, DCC-agnostic primitives. Stable API. Auto-loaded or shared. | `infrastructure` |
| **domain** | Business workflows for a specific DCC or task. Depends on infrastructure. | `domain` |
| **example** | Authoring references and demos. Never loaded in production. | `example` |

**Infrastructure skills** (e.g. `dcc-diagnostics`, `workflow`, `usd-tools`) are the
fallback and recovery layer. Every domain skill chains to them via `next-tools.on-failure`.

**Domain skills** (e.g. `maya-geometry`, `maya-pipeline`) implement a specific DCC
workflow. They declare `depends:` on the infrastructure skills they chain to, and their
tool descriptions guide agents toward other domain skills when the requested operation
is out of scope.

### Description pattern: explicit negative routing

The `description` field must follow a 3-part structure that tells agents both
when to use and when **not** to use the skill:

```
<Layer> skill — <one-sentence what + scope keywords>. Use when <trigger>.
Not for <counter-example> — use <other-skill> for that.
```

```yaml
# Infrastructure skill
description: >-
  Infrastructure skill — low-level OpenUSD scene inspection and validation:
  read layer stacks, traverse prims, validate schemas. Use when working
  directly with raw USD files. Not for Maya-specific USD export — use
  maya-pipeline__export_usd for that.

# Domain skill
description: >-
  Domain skill — Maya geometry primitives: create spheres, cubes, cylinders;
  bevel and extrude polygon components. Use for individual geometry operations
  in Maya. Not for full asset export pipelines — use maya-pipeline for that.
  Not for raw USD file inspection — use usd-tools for that.
```

### Metadata layer tag

Tag every skill with its layer so `search_skills` can filter by layer:

```yaml
metadata:
  dcc-mcp.layer: infrastructure   # or: domain | example
```

```python
# Browse only infrastructure skills
infra = catalog.search_skills(tags=["infrastructure"])

# Browse only domain skills for maya
domain = catalog.search_skills(tags=["domain"], dcc="maya")
```

### search-hint partitioning

Keep `search-hint` keywords non-overlapping across layers so `search_skills()`
returns the most relevant skill:

```yaml
# ✓ Infrastructure — mechanism-oriented (describes the tool/API itself)
dcc-mcp.search-hint: "usd stage, prim, schema validation, usdcat, usdchecker"

# ✓ Domain — intent-oriented (describes the user's goal)
dcc-mcp.search-hint: "export Maya scene to USD, asset pipeline, project setup"

# ✓ Example — append "authoring reference" to avoid production matches
dcc-mcp.search-hint: "async tool, deferred hint, authoring reference"
```

### Failure chain wiring

Every **domain skill tool** must wire `on-failure` to infrastructure diagnostics.
This gives the agent a consistent recovery path regardless of which domain skill failed:

```yaml
tools:
  - name: export_usd
    source_file: scripts/export_usd.py
    next-tools:
      on-success: [usd_tools__validate]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

Infrastructure skills do not need `on-failure` chains — they are the fallback.

### Checklist for a new domain skill

Before opening a PR for a new domain skill, verify:

- [ ] `metadata.dcc-mcp.layer: domain` is set
- [ ] `description` starts with `Domain skill —` and ends with at least one `Not for … — use … for that.` sentence
- [ ] `search-hint` uses intent-oriented keywords that do not overlap with infrastructure skills
- [ ] `depends:` lists every infrastructure skill referenced in `next-tools.on-failure`
- [ ] Every tool has `on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]`

## Complex Skill Architecture

The "Layered Skill Architecture" section above is about **discovery layers**
(`infrastructure` / `domain` / `example`) — how a skill is positioned for
agent routing. This section is about the **internal** organisation of the
files inside a single skill once it grows past one script.

### When to use a layered internal structure

The default single-file pattern (`scripts/execute.py`) is the right
starting point. Reach for an internal Tools / Services / Utils split when:

- multiple tools share orchestration logic,
- DCC commands need to be sequenced rather than called one-shot,
- helper functions deserve their own unit tests,
- one `execute.py` file has grown past ~200 lines.

Reference implementation:
[`examples/skills/example-layered-skill/`](https://github.com/loonghao/dcc-mcp-core/tree/main/examples/skills/example-layered-skill).

### Recommended layout

```text
my-complex-skill/
├── SKILL.md                ← agentskills.io frontmatter + prose
├── tools.yaml              ← MCP tool declarations (sibling, per #356)
├── scripts/
│   ├── __init__.py
│   ├── tools/              ← thin adapter layer (entry points)
│   │   ├── __init__.py
│   │   └── create_asset.py
│   ├── services/           ← business-logic layer (orchestration)
│   │   ├── __init__.py
│   │   └── asset_service.py
│   └── utils/              ← pure helpers (no I/O, no DCC calls)
│       ├── __init__.py
│       └── path_utils.py
└── prompts/
    └── system.md           ← optional system-prompt sidecar
```

### Layer responsibilities

| Layer | Responsibility | Imports allowed | Size guidance |
|-------|----------------|-----------------|---------------|
| `tools/` | Read JSON from stdin, validate, delegate, return `success/error` envelope. | `services/`, stdlib. | < 30 lines per file |
| `services/` | Orchestrate DCC commands. Raise typed exceptions on failure. No MCP knowledge. | `utils/`, DCC SDK. | grows with feature |
| `utils/` | Pure helpers — path/name normalisation, primitive math. No side effects. | stdlib only. | grows with feature |

### Wiring `source_file` to nested scripts

Because the SKILL.md scanner only auto-enumerates the **top level** of
`scripts/`, every tool whose entry point lives under `scripts/tools/`
must declare an explicit `source_file:` in `tools.yaml`:

```yaml
# tools.yaml
tools:
  - name: create_asset
    description: Create a new asset record on disk.
    source_file: scripts/tools/create_asset.py
    input_schema:
      type: object
      required: [name]
      properties:
        name: { type: string }
        kind: { type: string, default: model }
```

Relative `source_file` paths are resolved against the skill root.

### Cross-layer imports

Tool adapters need to import from sibling `services/` and `utils/`
packages. Add a small `sys.path` shim at the top of each tool entry
point so the imports work whether the script is run via the dcc-mcp-core
subprocess executor, an in-process executor, or directly with
`python scripts/tools/create_asset.py`:

```python
from pathlib import Path
import sys

_SCRIPTS_DIR = Path(__file__).resolve().parent.parent
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

from services.asset_service import AssetService  # noqa: E402
```

### Tool adapter template

```python
"""Tool entry point — create_asset (thin adapter)."""
from __future__ import annotations
import json, sys
from pathlib import Path

_SCRIPTS_DIR = Path(__file__).resolve().parent.parent
if str(_SCRIPTS_DIR) not in sys.path:
    sys.path.insert(0, str(_SCRIPTS_DIR))

from services.asset_service import AssetError, AssetService  # noqa: E402


def main() -> dict:
    params = json.loads(sys.stdin.read() or "{}")
    if not params.get("name"):
        return {"success": False, "message": "`name` is required"}
    try:
        asset = AssetService().create(name=params["name"], kind=params.get("kind", "model"))
    except AssetError as exc:
        return {"success": False, "message": str(exc)}
    return {
        "success": True,
        "message": f"Created asset {asset.id}",
        "context": {"asset_id": asset.id, "state": asset.state},
    }


if __name__ == "__main__":
    print(json.dumps(main()))
```

### Anti-patterns to avoid

- **Business logic in `tools/`** — adapters must stay under ~30 lines and
  only translate between MCP envelopes and service calls.
- **`utils/` doing I/O** — anything in `utils/` must be pure so it can be
  unit-tested without a DCC, a filesystem, or network access.
- **Cross-skill imports via relative paths** — share code by promoting it
  to its own infrastructure skill instead of `from ../other_skill ...`.
- **Returning envelopes from `services/`** — services raise typed
  exceptions; only the adapter wraps the outcome with `success_result()`
  / `error_result()`.
- **Auto-discovering nested scripts** — only top-level `scripts/*.py` are
  enumerated; nested entry points must be declared via `source_file`.

### Checklist for a new layered skill

- [ ] Entry points live under `scripts/tools/` and stay under ~30 lines
- [ ] Shared logic lives in `scripts/services/` and raises typed exceptions
- [ ] Pure helpers live in `scripts/utils/` and have no side effects
- [ ] Every tool in `tools.yaml` has an explicit `source_file:`
- [ ] Each adapter installs the `sys.path` shim shown above
- [ ] `metadata.dcc-mcp.tools: tools.yaml` is set in SKILL.md frontmatter

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

## Skill Directory Structure

Beyond the required `SKILL.md` and `scripts/`, the [agentskills.io](https://agentskills.io/specification) specification defines optional directories:

```
my-skill/
├── SKILL.md          # Required: metadata + instructions
├── scripts/          # Required: executable code
├── references/       # Optional: supplementary docs loaded on demand
│   ├── REFERENCE.md  # Detailed technical reference
│   └── FORMS.md      # Form templates or structured data formats
├── assets/           # Optional: templates, images, data files
└── metadata/         # Optional: dcc-mcp-core dependency declarations
    └── depends.md    # YAML list of dependency skill names
```

**`references/`** — Additional documentation that agents load on demand. Keep each file focused and small (< 2000 tokens recommended) to minimize context consumption. Reference files from SKILL.md body using relative paths: `See [reference guide](references/REFERENCE.md) for details.`

**`assets/`** — Static resources like document templates, configuration templates, images, or lookup tables. Not automatically loaded; agents access them when needed.

> **Tip**: Keep the main `SKILL.md` body under **500 lines** and **5000 tokens**. Move detailed reference material to `references/` files — agents load them only when needed (progressive disclosure).

## SkillMetadata Fields

Parsed from SKILL.md frontmatter. Supports [agentskills.io](https://agentskills.io/specification) standard fields, ClawHub/OpenClaw, and dcc-mcp-core extensions simultaneously.

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Unique skill name |
| `description` | `str` | Short description (should describe what the skill does and when to use it) |
| `search_hint` | `str` | Keyword hint for `search_skills` (SKILL.md `search-hint:` field; falls back to `description`) |
| `tools` | `List[ToolDeclaration]` | Tool declarations from frontmatter (use `.name` to get tool names) |
| `dcc` | `str` | Target DCC application (default: `"python"`) |
| `tags` | `List[str]` | Classification tags |
| `scripts` | `List[str]` | Discovered script file paths |
| `skill_path` | `str` | Absolute path to the skill directory |
| `version` | `str` | Skill version (default: `"1.0.0"`) |
| `depends` | `List[str]` | Skill dependency names |
| `metadata_files` | `List[str]` | Paths to `.md` files in `metadata/` |
| `groups` | `List[SkillGroup]` | Tool groups for progressive exposure (see below) |
| `license` | `str` | License identifier (agentskills.io spec, e.g. `"MIT"`, `"Apache-2.0"`) |
| `compatibility` | `str` | Environment requirements, max 500 chars (agentskills.io spec) |
| `allowed_tools` | `List[str]` | Pre-approved tools (agentskills.io spec, experimental) |
| `external_deps` | `str \| None` | External dependency declaration as JSON string (MCP servers, env vars, binaries). Set via `md.external_deps = json.dumps(deps)`, read via `json.loads(md.external_deps)`. See [Skill Scopes & Policies](skill-scopes-policies.md) for the full schema. |

## Tool Groups (Progressive Exposure)

Large skills often expose far more tools than an AI client needs at any given
moment. Tool groups let a skill ship several related toolsets and let the
client activate only the ones it needs — keeping `tools/list` small while all
tools remain discoverable.

### Declaring Groups in SKILL.md

Groups are declared in the top-level `groups:` section. Each tool can then
reference a group name via its `group:` field:

```yaml
---
name: maya-geometry
description: "Maya geometry, modeling, and rigging tools"
dcc: maya
groups:
  - name: modeling
    description: "Polygon modeling and UV tools"
    default_active: true          # active at load time (no activation needed)
    tools: [create_sphere, create_cube, extrude]
  - name: rigging
    description: "Skeleton, joints and skinning"
    default_active: false         # inactive until activate_tool_group is called
    tools: [create_joint]
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    group: modeling
    source_file: scripts/create_sphere.py
  - name: create_joint
    description: "Create a joint chain"
    group: rigging
    source_file: scripts/create_joint.py
---
```

### `SkillGroup` Fields

| Field | Type | Default | Description |
|-------|------|---------|-------------|
| `name` | `str` | required | Group identifier (kebab-case recommended) |
| `description` | `str` | `""` | Human-readable description |
| `default_active` | `bool` | `False` | Active immediately after `load_skill`; `False` means dormant until activated |
| `tools` | `List[str]` | `[]` | Tool names in this group — must match `ToolDeclaration.name` entries |

### How Groups Behave After `load_skill`

When `SkillCatalog.load_skill("maya-geometry")` runs:

1. All tool declarations are registered in `ToolRegistry` with their
   group metadata set to the declared group name.
2. Tools in groups where `default_active=false` are hidden from
   `tools/list`. They remain in the registry (visible via `list_actions()`)
   and become active once the group is activated.
3. `SkillCatalog.active_groups(skill_name)` returns the initially-active groups.

### Controlling Groups at Runtime

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry, create_skill_server

# Via the high-level catalog
catalog.activate_group("maya-geometry", "rigging")     # enables rigging tools
catalog.deactivate_group("maya-geometry", "rigging")   # disables them again
groups   = catalog.list_groups("maya-geometry")         # -> List[SkillGroup]
tools    = catalog.list_tools_catalog("maya-geometry")  # group -> tools map

# Or via the registry directly (emits tools/list_changed notifications)
registry.activate_tool_group("maya-geometry", "rigging")
registry.deactivate_tool_group("maya-geometry", "rigging")
enabled_tools = registry.list_tools_in_group("maya-geometry", "modeling")
enabled_only  = registry.list_actions_enabled()
```

### MCP Tools for Group Management

`create_skill_server` / `McpHttpServer` register three core MCP tools for
group control in addition to the six skill-discovery tools:

| Tool | Description |
|------|-------------|
| `activate_tool_group` | Activate a group; emits `notifications/tools/list_changed` so clients refresh their view |
| `deactivate_tool_group` | Deactivate a group |
| `search_tools` | Keyword search across currently-enabled tools (name, description, tags) |

`tools/list` also returns `__group__<skill>.<group>` stubs for any group
that is inactive, making the full tool surface discoverable without
exposing schemas or handlers.

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

1. **5 core discovery tools** (always present):
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

Searches across: `name`, `description`, `search_hint`, `tags`, `dcc`, and the
sibling `tools.yaml` entries (`name` + `description`). The `search_hint` field
(from SKILL.md `search-hint:`) improves keyword matching without loading full
schemas.

#### How `search_skills` ranks results

Since dcc-mcp-core 0.15 (issue [#343](https://github.com/loonghao/dcc-mcp-core/issues/343))
the ranker is a tokenised BM25-lite scorer. It is deterministic — the same
skill set + query always yields the same order.

**Tokeniser** — lowercase, split on `[\s_\-.,;:/]+`, drop a small stopword
list (`a, an, the, of, and, or, to, for, with, from`). No stemming, no
fuzzy match.

**Field weights**

| Field | Weight |
|-------|-------:|
| `name`                                   | 5.0 |
| `dcc` (exact token match only)           | 4.0 |
| `tags`                                   | 3.0 |
| `search_hint`                            | 3.0 |
| `description`                            | 2.0 |
| sibling tool names (from `tools.yaml`)   | 2.0 |
| sibling tool descriptions                | 1.0 |

**Scoring** — standard BM25 per query token with `k1=1.2`, `b=0.75`, document
length = total tokens across all weighted fields. The per-field contributions
are multiplied by the weights above and summed across query tokens.

**Tie-breaks** (in order)

1. **Exact-name fast-path** — if the query equals a skill's name
   (case-insensitive, after trimming), that skill sorts first unconditionally.
2. **Name-substring hit** — skills whose `name` contains the query as a
   raw substring rank above equal-scoring skills that don't.
3. **Scope precedence** — `Admin > System > User > Repo`.
4. **Alphabetical name**.

Skills with a total score of `0.0` (and no exact-name match) are dropped.
The `tags` and `dcc` filter arguments are applied *before* scoring.

`create_skill_server()` only calls `discover()` at startup — skills are **not** automatically loaded. This keeps the initial tool list small and lets agents load only what they need.

## Migrating pre-0.15 SKILL.md

Starting with dcc-mcp-core 0.15 (issue [#356](https://github.com/loonghao/dcc-mcp-core/issues/356)), dcc-mcp-core-specific extension keys (`dcc`, `version`, `tags`, `tools`, …) should live under the agentskills.io-compliant `metadata.dcc-mcp.*` namespace rather than at the top level of SKILL.md frontmatter. Top-level extension keys continue to parse but emit a one-shot deprecation warning per skill, and `SkillMetadata.is_spec_compliant()` returns `False` for them.

### Before (pre-0.15, legacy — still works, now deprecated)

```yaml
---
name: maya-geometry
description: "Maya geometry creation and modification tools"
dcc: maya
version: "1.0.0"
tags: [geometry, create]
search-hint: "polygon modeling, sphere, bevel, extrude"
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    source_file: scripts/create_sphere.py
---
```

### After (0.15+, agentskills.io 1.0 compliant)

```yaml
---
name: maya-geometry
description: "Maya geometry creation and modification tools"
license: MIT
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "1.0.0"
  dcc-mcp.tags: "geometry, create"
  dcc-mcp.search-hint: "polygon modeling, sphere, bevel, extrude"
  dcc-mcp.tools: tools.yaml     # sibling file (relative to SKILL.md)
---
```

Then put the MCP tool declarations in a sibling `tools.yaml`:

```yaml
# tools.yaml — same schema as the inline tools: block
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    source_file: scripts/create_sphere.py
groups:
  - name: advanced
    default-active: false
    tools: [create_sphere]
```

#### Declaring tool annotations (issue #344)

Each tool entry in `tools.yaml` can carry MCP
[`ToolAnnotations`](https://modelcontextprotocol.io/specification/2025-03-26/server/tools#tool-annotations)
that describe how the tool behaves. dcc-mcp-core surfaces them on
`tools/list` with spec-compliant camelCase keys
(`readOnlyHint`, `destructiveHint`, `idempotentHint`, `openWorldHint`).
Tools without any declared annotations **omit** the `annotations` field
entirely — no empty object.

Two declaration forms are accepted:

**1. Canonical — nested `annotations:` map (preferred).**

```yaml
# tools.yaml
tools:
  - name: delete_keyframes
    description: Delete keyframes in a frame range
    annotations:
      read_only_hint: false
      destructive_hint: true
      idempotent_hint: true
      open_world_hint: false
```

**2. Shorthand — flat hint keys on the tool entry (backward compatibility).**

```yaml
tools:
  - name: get_keyframes
    read_only_hint: true
    idempotent_hint: true
```

**Precedence: the nested `annotations:` map wins whole-map when both
forms are present for the same tool** — not a per-field merge. This
avoids confusing precedence when authors migrate from shorthand to
canonical:

```yaml
tools:
  - name: risky
    read_only_hint: true         # ignored (shorthand)
    idempotent_hint: true        # ignored (shorthand)
    annotations:
      destructive_hint: true     # wins — only this hint is surfaced
```

**`deferred_hint` is a dcc-mcp-core extension, not MCP 2025-03-26.**
When you declare `deferred_hint: true`, it rides in
`_meta["dcc.deferred_hint"]` on the tool declaration — never inside the
spec-standard `annotations` map (which would make the payload
non-compliant). The same `_meta` slot also carries the
`execution: async` implication from issue #317.

#### Declaring `next-tools` (issue #342)

`next-tools` belongs **inside each tool entry in `tools.yaml`** — never as
a top-level SKILL.md frontmatter key. The server surfaces the declared
list on `CallToolResult._meta["dcc.next_tools"]`:

- `on-success` list → attached after a successful tool call
- `on-failure` list → attached after an error (`isError == true`)
- No `next-tools` declared → `_meta["dcc.next_tools"]` is omitted entirely

```yaml
# tools.yaml
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    source_file: scripts/create_sphere.py
    next-tools:
      on-success:
        - maya_geometry__bevel_edges
        - maya_geometry__assign_material
      on-failure:
        - diagnostics__screenshot
        - diagnostics__audit_log
```

Tool names are validated with `dcc_mcp_naming::validate_tool_name` at
skill-load time. Invalid entries are dropped with a `tracing::warn!`
and the rest of the skill loads normally — a single malformed name
will not fail the whole skill.

A legacy top-level `next-tools:` block on SKILL.md still parses for
backward compatibility but emits a deprecation warning and flips
`SkillMetadata.is_spec_compliant()` to `False` (`next-tools` appears
in `legacy_extension_fields`).

### Metadata key reference

| Legacy top-level                | Spec-compliant `metadata` key                | Value type            |
| ------------------------------- | -------------------------------------------- | --------------------- |
| `dcc: maya`                     | `metadata["dcc-mcp.dcc"]`                    | string                |
| `version: 1.0.0`                | `metadata["dcc-mcp.version"]`                | string                |
| `tags: [a, b]`                  | `metadata["dcc-mcp.tags"]`                   | comma-separated string |
| `search-hint: "…"`              | `metadata["dcc-mcp.search-hint"]`            | string                |
| `depends: [x, y]`               | `metadata["dcc-mcp.depends"]`                | comma-separated string |
| `products: [maya]`              | `metadata["dcc-mcp.products"]`               | comma-separated string |
| `allow_implicit_invocation`     | `metadata["dcc-mcp.allow-implicit-invocation"]` | `"true"` / `"false"` |
| `external_deps: {...}`          | `metadata["dcc-mcp.external-deps"]`          | JSON string           |
| `tools: [...]` inline block     | `metadata["dcc-mcp.tools"]`                  | sibling `.yaml` file  |
| `groups: [...]` inline block    | `metadata["dcc-mcp.groups"]`                 | sibling `.yaml` file  |
| *(MCP prompts primitive)*       | `metadata["dcc-mcp.prompts"]`                | sibling `.yaml` file or `prompts/*.prompt.yaml` glob |

### Priority rules

- When both forms are present, the `metadata.dcc-mcp.*` value wins.
- If only the legacy top-level field is present, it is still read (backward compatibility) and the loader emits a single `tracing::warn!` per skill.
- Checking compliance programmatically:

  ```python
  skills, _skipped = dcc_mcp_core.scan_and_load(dcc_name="maya")
  for s in skills:
      if not s.is_spec_compliant():
          print(f"{s.name}: legacy fields={s.legacy_extension_fields}")
  ```

A one-shot CLI migrator (`dcc-mcp-migrate-skill`) is planned as a follow-up; see the tracking issue for status.

### The sibling-file pattern is the rule for every new extension

The migration table above is not an exhaustive list — it is an example
of **the single design rule** that governs every SKILL.md extension
dcc-mcp-core adds from v0.15 onward:

> **New extensions live under `metadata.dcc-mcp.<feature>` and point at sibling files; they never add new top-level frontmatter keys and they never inline large payloads into SKILL.md.**

Concrete applications (shipped or in flight):

| Feature | Metadata key | Sibling file(s) | Issue |
|---|---|---|---|
| Tool declarations + groups | `metadata["dcc-mcp.tools"]`, `metadata["dcc-mcp.groups"]` | `tools.yaml` | #356 |
| Workflow specs | `metadata["dcc-mcp.workflows"]` | `workflows/*.workflow.yaml` | #348 |
| Prompts / templates | `metadata["dcc-mcp.prompts"]` | `prompts/*.prompt.yaml` | #351, #355 |
| Example dialogues | `metadata["dcc-mcp.examples"]` | `references/EXAMPLES.md` or `examples/*.md` | (future) |
| Tool annotation packs | `metadata["dcc-mcp.annotations"]` | `annotations.yaml` or carried in `tools.yaml` | #344 |
| next-tools behaviour chains | (carried inline inside `tools.yaml`) | n/a — never a top-level SKILL.md field | #342 |

Layout convention (pick the shape that matches the feature's cardinality):

```
my-skill/
├── SKILL.md
├── tools.yaml               # one file — tools + groups + next-tools
├── workflows/               # many files — one per workflow
│   ├── vendor_intake.workflow.yaml
│   └── nightly_cleanup.workflow.yaml
├── prompts/                 # many files — one per prompt template
│   └── review_scene.prompt.yaml
└── references/              # Markdown reference material (agentskills.io standard)
    ├── EXAMPLES.md
    └── REFERENCE.md
```

Why the rule holds:

- **Spec-conformance**: `skills-ref validate` (agentskills.io's
  reference validator) passes without a custom ruleset.
- **Progressive disclosure**: `search_skills` reads only SKILL.md; a
  sibling file loads on demand when the agent activates that feature.
- **Bounded SKILL.md**: the body stays ≤500 lines / ≤5000 tokens
  regardless of how many workflows / prompts / examples ship.
- **Diff-friendly**: a new workflow is one new YAML file in review,
  not a 300-line diff inside SKILL.md.

If a feature you are designing can't fit this pattern, that is a
signal to write a proposal under `docs/proposals/` and discuss the
frontmatter impact before implementing.

## Validating Skills

Use `validate_skill` to check a skill directory against the specification before loading it at runtime. This catches structural errors, missing files, and format violations early.

```python
from dcc_mcp_core import validate_skill

report = validate_skill("/path/to/my-skill")

if report.is_clean:
    print("Skill is valid!")
else:
    for issue in report.issues:
        print(f"[{issue.severity}] {issue.category}: {issue.message}")
```

### Validation Rules

The validator checks the following categories:

| Category | Checks |
|----------|--------|
| **SkillMd** | `SKILL.md` exists and is readable |
| **Frontmatter** | YAML is well-formed; required fields (`name`, `description`) present; `name` is kebab-case and ≤64 chars; `description` ≤1024 chars; `compatibility` ≤500 chars |
| **Tools** | Tool names are non-empty, unique, and snake_case; descriptions are present; `group` references exist in declared groups; `next-tools` references point to existing tools |
| **Scripts** | `source_file` references exist in `scripts/`; file extensions are supported |
| **Sidecars** | `metadata.dcc-mcp.tools/groups/prompts` sibling files exist |
| **Dependencies** | `depends` entries are non-empty; `metadata/depends.md` exists when `depends` is declared |

### Severity Levels

- **Error** — the skill cannot be loaded or will malfunction
- **Warning** — the skill loads but violates a best-practice or spec recommendation
- **Info** — purely informational (e.g. legacy extension field deprecation notice)

### Using the `dcc-skills-creator` Skill

The `examples/skills/dcc-skills-creator/` skill provides scaffolding and validation helpers:

```python
# Scaffold a new skill directory
from dcc_mcp_core.skill import create_skill

skill_path = create_skill("my-new-skill", "/path/to/skills", dcc="maya")
# Creates: my-new-skill/SKILL.md, scripts/, metadata/

# Get a full SKILL.md template
from dcc_mcp_core.skill import skill_template
print(skill_template())
```
