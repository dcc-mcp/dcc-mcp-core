# Skills System

The Skills system registers scripts (Python, MEL, MaxScript, BAT, Shell, etc.) as MCP-discoverable tools with **zero Python glue code**. `SKILL.md` follows the agentskills.io V1.0 frontmatter shape; dcc-mcp-core extensions live under `metadata.dcc-mcp.*` and point to sibling files such as `tools.yaml` and `groups.yaml`. Keep extension data out of top-level `SKILL.md` keys so generic agentskills.io readers can still parse the package.

For **adapter maintenance** (I/O tool copy, `recipes` / `skill-reference-docs`, gateway-friendly descriptions), use [skill-maintenance.md](skill-maintenance.md). For agent-facing adapter skill development guidance, load `skills/dcc-mcp-skills-creator/`. In-tree reference skills: `python/dcc_mcp_core/skills/dcc-diagnostics`, `python/dcc_mcp_core/skills/workflow`.

## Quick Start

### 1. Create a Skill Directory

```
maya-geometry/
├── SKILL.md
├── tools.yaml
└── scripts/
    ├── create_sphere.py
    └── export_fbx.bat
```

### 2. Write `SKILL.md` and `tools.yaml`

`SKILL.md`:

```yaml
---
name: maya-geometry
description: >-
  Domain skill — Maya geometry creation and modification tools.
  Use when the user asks to create, inspect, or export polygon meshes in Maya.
  Not for render-farm submission — use maya-render-farm for that.
license: MIT
compatibility: maya>=2022
metadata:
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    layer: domain
    tags: [geometry, create]
    search-hint: "polygon modeling, sphere, bevel, extrude, mesh editing"
    search-aliases: [primitive ball, mesh globe]
    tools: tools.yaml
---
# Maya Geometry Skill

Use these tools to create and modify geometry in Maya.
```

```yaml
# tools.yaml
tools:
  - name: create_sphere
    description: Create a polygon sphere with the given radius.
    search_aliases: [primitive ball, mesh globe]
    source_file: scripts/create_sphere.py
    annotations:
      read_only_hint: false
      destructive_hint: false
      idempotent_hint: false
    next-tools:
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
  - name: export_fbx
    description: Export selected objects to FBX.
    source_file: scripts/export_fbx.bat
```

The `metadata.dcc-mcp.search-hint` field provides comma-separated keywords for efficient skill discovery via `search_skills` without loading full tool schemas. Use bounded `metadata.dcc-mcp.search-aliases` and per-tool `search_aliases` in `tools.yaml` for domain synonyms, localized terms, or common user phrases that should improve gateway/per-DCC search recall without changing tool names, summaries, tags, or dispatch inputs.

Optional adapter runtimes can be declared without turning skill discovery into
an installer or shell executor. Use inline `metadata.dcc-mcp.runtimes`, or point
that field at a sibling `runtimes.yaml`, for safe probes that run during
search/detail: environment variables are checked for non-empty values, binaries
are resolved on `PATH`, and Python packages use `importlib.util.find_spec()`
without importing the package or executing tool scripts.

```yaml
metadata:
  dcc-mcp:
    runtimes:
      - name: usd-core
        type: python_package
        package: usd-core
        module: pxr
        optional: true
        feature_level: full-usd
        install_hint: "pip install dcc-mcp-openusd[usd-core]"
      - name: usdcat
        type: binary
        binary: usdcat
        optional: true
        feature_level: usd-cli
        guidance: "Install OpenUSD command-line tools to enable USD CLI checks."
      - name: houdini-solaris
        type: env_var
        env: HFS
        optional: true
        feature_level: solaris
        guidance: "Start from a Houdini environment or set HFS."
```

Runtime states are `available`, `degraded`, and `missing`. Optional missing
runtimes report `degraded`, while required missing runtimes report `missing`.
`search_skills()`, `list_skills()`, `get_skill_info()`, gateway search, and REST
describe surfaces expose the resolved state so agents can decide whether to
load/call a skill before invoking any adapter code.

### Preferred Runtime Helpers

New skill scripts should import dependency-light helper APIs from
`dcc_mcp_core.skills_helper` before adding small Python dependencies or local
utility modules:

```python
from dcc_mcp_core.skills_helper import (
    SkillFileError,
    SkillHttpError,
    ToolValidator,
    atomic_write_text,
    http_get_json,
    json_dumps,
    json_loads,
    load_yaml_file,
    normalize_tool_arguments,
    skill_entry,
    skill_error_from_exception,
    skill_success,
    yaml_dumps,
    yaml_loads,
)
```

Use `json_dumps` / `json_loads` and `yaml_dumps` / `yaml_loads` instead of
adding PyYAML or local JSON/YAML wrappers for ordinary skill payloads. Use
`http_request`, `http_get_json`, and `http_post_json` for bounded synchronous
JSON REST calls; always set `timeout_ms` and `max_bytes`, and redact headers
before surfacing them in errors or audit metadata. Use file/path helpers such
as `ensure_within_root`, `atomic_write_text`, `load_json_file`,
`load_yaml_file`, and `file_digest` for local session files. Use
`ToolValidator`, `normalize_tool_arguments`, result helpers, and cancellation
checks from the same namespace so scripts do not mix helper import paths.

Keep a domain-specific dependency only when it owns behavior that
`skills_helper` intentionally does not cover: sessions, streaming, multipart
upload, custom retry/auth flows, SDK-specific API models, non-JSON protocols,
or rich domain file formats. Existing top-level imports such as
`from dcc_mcp_core import json_dumps` remain supported for compatibility, but
new docs, generators, and templates should show `dcc_mcp_core.skills_helper`
as the canonical path.

When migrating old skills, replace one concern at a time and keep the old test
fixtures running:

1. Replace direct `requests.get(...).json()` calls with `http_get_json(...)`
   where the call is a simple bounded JSON request.
2. Replace PyYAML or local YAML helpers with `load_yaml_file(...)` /
   `yaml_loads(...)` for bounded configs.
3. Replace ad-hoc path containment and temp-file writes with
   `ensure_within_root(...)` and `atomic_write_text(...)`.
4. Re-run `dcc_mcp_skills_creator__validate_skill_dir`; it now reports
   `skill-helper-adoption` warnings for generated/reference skills importing
   avoidable dependencies such as `requests`, `httpx`, PyYAML, or local helper
   modules covered by `skills_helper`.

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

# Adapter-side runtime policy changes
skill = catalog.get_skill("maya-geometry")  # detached SkillMetadata copy
for tool in skill.tools:
    if tool.thread_affinity == "main":
        tool.enforce_thread_affinity = False
catalog.load_skill_object(skill)  # registers through the normal core path
```

Use `get_skill()` / `load_skill_object()` when an adapter must adjust skill
metadata at runtime before registration, for example standalone/headless
compatibility for main-thread tools. Do not parse or rewrite `SKILL.md` or
`tools.yaml` from adapter code for those runtime overrides. `get_skill_info()`
remains the serialized inspection view; mutating the returned dict does not
change catalog state.

### SkillSummary Fields

`search_skills()` and `list_skills()` return `SkillSummary` objects:

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Skill name |
| `description` | `str` | Short description |
| `search_hint` | `str` | Keyword hint for discovery (from `metadata.dcc-mcp.search-hint`; falls back to `description`) |
| `tags` | `List[str]` | Skill tags |
| `dcc` | `str` | Target DCC (e.g. `"maya"`) |
| `version` | `str` | Skill version |
| `tool_count` | `int` | Number of declared tools |
| `tool_names` | `List[str]` | Names of declared tools |
| `loaded` | `bool` | Whether the skill is currently loaded |
| `scope` | `str` | Trust scope such as `repo`, `user`, `system`, or `admin` |
| `implicit_invocation` | `bool` | Whether tools may be invoked without an explicit `load_skill` step |
| `runtime` | `SkillRuntimeSummary \| None` | Aggregate optional runtime state from `metadata.dcc-mcp.runtimes` |

## ToolDeclaration

A `ToolDeclaration` describes a single tool within a skill. Declare tools in the sibling `tools.yaml` file referenced by `metadata.dcc-mcp.tools`. Legacy top-level `tools:` frontmatter is rejected by the strict loader; migrate old skills to the sibling-file pattern. Schema keys accept both dcc-mcp-core snake_case (`input_schema`, `output_schema`) and MCP-style camelCase (`inputSchema`, `outputSchema`) so authors can copy schemas from MCP tooling without renaming them:

```yaml
tools:
  - name: create_sphere
    description: "Create a polygon sphere"
    inputSchema:
      type: object
      properties:
        radius:
          type: number
          description: Sphere radius in scene units
    search_aliases: [primitive ball, mesh globe]
    outputSchema:
      type: object
      properties:
        name:
          type: string
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
| `input_schema` | `str` (JSON) | `None` | JSON Schema for input parameters; YAML also accepts `inputSchema` |
| `output_schema` | `str` (JSON) | `None` | JSON Schema for output; YAML also accepts `outputSchema` |
| `search_aliases` | `List[str]` | `[]` | Bounded search-only synonyms. These are indexed for discovery with schema field names, but are not tool arguments and are not a replacement for a concise description. |
| `read_only` | `bool` | `False` | Whether this tool only reads data |
| `destructive` | `bool` | `False` | Whether this tool may cause destructive changes |
| `idempotent` | `bool` | `False` | Whether calling with same args always produces same result |
| `defer_loading` | `bool` | `False` | Accepts `defer-loading` / `defer_loading` in SKILL.md and marks the declaration as discovery-oriented |
| `source_file` | `str` | `""` | Explicit path to the script (relative to skill dir) |

### Deriving `input_schema` / `output_schema` from Python types (#242)

Hand-writing JSON Schema is error-prone — agent-authored actions drift,
cached schemas diverge from runtime code, and the schema text is noisy
to review. `dcc_mcp_core.schema` derives both schemas from Python type
annotations using only the standard library (no `pydantic`, no
`jsonschema`, no `attrs`).

Write a typed handler:

```python
from dataclasses import dataclass, field
from typing import Tuple

from dcc_mcp_core import tool_spec_from_callable
from dcc_mcp_core._tool_registration import register_tools


@dataclass
class ExportInput:
    scene_path: str = field(metadata={"description": "Scene file to export."})
    format: str = "fbx"
    frame_range: Tuple[int, int] = (1, 100)


@dataclass
class ExportResult:
    path: str
    size_bytes: int
    took_ms: int


def export_scene(args: ExportInput) -> ExportResult:
    """Export a scene to an interchange format."""
    ...


spec = tool_spec_from_callable(export_scene)
register_tools(server, [spec], dcc_name="maya")
```

`tool_spec_from_callable` understands two signature styles:

- **Single dataclass / TypedDict parameter** — the whole parameter becomes
  the `inputSchema` (used above).
- **Multiple primitive-typed parameters** — each parameter becomes a
  property of an `object` `inputSchema`.

The return annotation, when present, becomes the `outputSchema` so MCP
2025-06-18 clients can validate `structuredContent` payloads. Untyped
handlers raise `TypeError` rather than silently falling back to a
permissive `{"type": "object"}` — this closes the #588-era footgun.

Supported types (stdlib only): `bool`, `int`, `float`, `str`, `bytes`,
`None`, `list[X]`, `tuple[X, ...]`, `tuple[A, B, ...]` (fixed),
`dict[str, V]`, `Optional[X]` / `X | None`, `Union[A, B]`,
`Literal[...]`, `Enum`, `datetime.datetime`, `datetime.date`,
`pathlib.Path`, `uuid.UUID`, `@dataclass`, `TypedDict`. On Python 3.7,
spell containers and unions with `typing.List`, `typing.Dict`,
`typing.Tuple`, `typing.Optional`, and `typing.Union`; `Literal` and
`TypedDict` require `typing_extensions` in the skill author's environment.
The core package still imports without third-party Python library dependencies. Unsupported
types raise `TypeError` with a clear escape hatch: pass an explicit
`input_schema=...` dict or use pydantic's `MyModel.model_json_schema()`.

::: tip Why not pydantic?
We intentionally stay free of third-party Python library dependencies. Adding `pydantic` for this one
feature would drag in a 3MB wheel plus `pydantic-core` and is too
heavy for authors who only want a few dataclass handlers. For callers
who already use pydantic, the emitted shape matches pydantic's
conventions (`title`, `$defs`, `$ref`, `anyOf`, `required`), so
swapping in `MyModel.model_json_schema()` is a drop-in replacement.

`pydantic-core` (Rust) was evaluated for reuse and doesn't help here:
pydantic's own architecture doc confirms JSON Schema generation lives
entirely in the Python package — the Rust crate only runs validation
and serialization, never introspecting Python type annotations.
:::

See `examples/skills/typed-schema-demo/` for a runnable example, and
`tests/test_schema.py` for the full type-mapping table in executable
form.

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

Both accept lists of fully-qualified tool names in `{skill_name}__{tool_name}` format. Dotted MCP tool names are not valid; see [Naming Rules](/guide/naming) for validation.

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
skills_lenient, skipped = scan_and_load_lenient(dcc_name="maya")  # keep soft-dep skills discoverable

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
- `scan_and_load` — fail-fast on missing dependencies or cycles after logging
  skipped directories and parse failures. Good for CI and packaged adapter
  releases where dependency completeness should block the build.
- `scan_and_load_lenient` — same return shape but keeps skills with missing
  soft dependencies discoverable; present dependencies still sort before their
  dependents, and dependency cycles still fail.
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

### The four layers

| Layer | Role | `dcc-mcp.layer` value | Search rank |
|-------|------|----------------------|-------------|
| **domain** | Business workflows for a specific DCC or task. Depends on infrastructure. | `domain` | × 1.00 |
| **infrastructure** | Low-level, DCC-agnostic primitives. Stable API. Auto-loaded or shared. | `infrastructure` | × 0.35 |
| **thin-harness** | One Python script + minimal SKILL.md — raw `python` / `bash` / CLI wrappers. | `thin-harness` | × 0.20 |
| **example** | Authoring references and demos. Never loaded in production. | `example` | **excluded** |

**Domain skills** (e.g. `maya-geometry`, `maya-pipeline`) implement a specific DCC
workflow. They declare `metadata.dcc-mcp.depends` for the infrastructure skills
they chain to, and their tool descriptions guide agents toward other domain
skills when the requested operation is out of scope.

**Infrastructure skills** (e.g. `dcc-diagnostics`, `workflow`, `usd-tools`) are the
fallback and recovery layer. Every domain skill chains to them via `next-tools.on-failure`.

**Thin-harness skills** are the lowest tier that still appears in search results.
They wrap a raw script with minimal SKILL.md boilerplate — useful as a last-resort
escape hatch when no domain or infrastructure skill covers the needed operation.

**Example skills** are dropped from search results entirely. They only surface when
the caller explicitly asks for them via `search_skills(tags=["example"])` or types
the exact skill name.

::: tip Bypassing penalties
The layer penalty and the `example` exclusion are bypassed when the caller filters
by a known layer name through `tags=` (case-insensitive), e.g.
`search_skills(tags=["thin-harness"])` or `search_skills(tags=["infrastructure"])`.
The raw BM25 order is honoured inside the filtered slice.
:::

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

### Skill path-source rank

A second multiplier is layered on top of the layer multiplier based on
where the skill was discovered from. User-curated locations rank at
parity (× 1.00); bundled / platform-installed starter material is
slightly damped so a local-dev skill always wins a tie.

| Source | Where it comes from | Rank |
|--------|---------------------|------|
| `ExplicitArg` | `extra_paths` passed to `discover()` / `scan_*` | × 1.00 |
| `AdminCustom` | Added through the admin UI (gateway SQLite lane) | × 1.00 |
| `EnvVar` | `DCC_MCP_SKILL_PATHS` / `DCC_MCP_<APP>_SKILL_PATHS` | × 1.00 |
| `LocalDev` | `~/.dcc-mcp/<dcc>/skills` (local iteration root) | × 1.00 |
| `Platform` | Platform-wide install dir (`get_skills_dir`) | × 0.85 |
| `Bundled` | Shipped with the dcc-mcp package itself | × 0.70 |

The path-source multiplier compounds multiplicatively with the layer
multiplier (both are ≤ 1.00). The exact-name fast-path bypasses it —
`search_skills("dcc-diagnostics")` still surfaces a bundled diagnostics
skill at the top regardless of source.

Source tagging happens at scan time in
`crates/dcc-mcp-skills/src/scanner.rs::scan_with_sources`. Adapters can
read the assigned source via `SkillEntry.path_source` for diagnostic
surfaces (e.g. the admin UI's skill panel) — it is stable across
restarts because the field is `#[serde(default)]`.

### Metadata layer tag

Tag every skill with its layer so `search_skills` can filter by layer:

```yaml
metadata:
  dcc-mcp:
    layer: domain   # or: infrastructure | thin-harness | example
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
# Infrastructure — mechanism-oriented (describes the tool/API itself)
metadata:
  dcc-mcp:
    search-hint: "usd stage, prim, schema validation, usdcat, usdchecker"

# Domain — intent-oriented (describes the user's goal)
metadata:
  dcc-mcp:
    search-hint: "export Maya scene to USD, asset pipeline, project setup"

# Example — append "authoring reference" to avoid production matches
metadata:
  dcc-mcp:
    search-hint: "async tool, deferred hint, authoring reference"
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
- [ ] `metadata.dcc-mcp.depends` lists every infrastructure skill referenced in `next-tools.on-failure`
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
[`examples/skills/example-layered-skill/`](https://github.com/dcc-mcp/dcc-mcp-core/tree/main/examples/skills/example-layered-skill).

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
- [ ] `metadata.dcc-mcp.tools` is set to `tools.yaml` in nested SKILL.md frontmatter

## Dependency Resolution

Skills can declare dependencies on other skills using `metadata.dcc-mcp.depends` in SKILL.md or the sibling `metadata/depends.md` file:

```yaml
---
name: maya-animation
metadata:
  dcc-mcp:
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

Dependency declarations are soft during catalog discovery. If a composition
skill depends on `maya-dev` before that skill has been discovered, the
composition skill still appears in `search_skills()` / `list_skills()` with
`status: "pending_deps"` and a `missing_dependencies` list. Calling
`load_skill("maya-animation")` auto-loads any discovered dependencies first;
when a dependency is still missing, `load_skill` returns an actionable error
that names the missing skill and asks the caller to discover or install it.
Use `scan_and_load()` / `resolve_dependencies()` when you explicitly want a
fail-fast dependency check for packaging or CI.

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

Parsed from agentskills.io `SKILL.md` frontmatter plus sibling files referenced by `metadata.dcc-mcp.*`. Top-level extension keys are rejected by the strict loader; keep dcc-mcp-core payloads in the namespace and sibling files.

| Field | Type | Description |
|-------|------|-------------|
| `name` | `str` | Unique skill name |
| `description` | `str` | Short description (should describe what the skill does and when to use it) |
| `search_hint` | `str` | Keyword hint for `search_skills` (`metadata.dcc-mcp.search-hint`; falls back to `description`) |
| `tools` | `List[ToolDeclaration]` | Tool declarations from `metadata.dcc-mcp.tools` sibling file (use `.name` to get tool names) |
| `dcc` | `str` | Target DCC application (default: `"python"`) |
| `tags` | `List[str]` | Classification tags |
| `scripts` | `List[str]` | Discovered script file paths |
| `skill_path` | `str` | Absolute path to the skill directory |
| `version` | `str` | Skill version (default: `"1.0.0"`) |
| `depends` | `List[str]` | Skill dependency names |
| `metadata_files` | `List[str]` | Paths to `.md` files in `metadata/` |
| `groups` | `List[SkillGroup]` | Tool groups for progressive exposure (see below) |
| `runtimes` | `List[SkillRuntimeDescriptor]` | Optional runtime descriptors from inline `metadata.dcc-mcp.runtimes` or a sibling `runtimes.yaml`; resolved into discovery/detail runtime state without executing tool scripts |
| `license` | `str` | License identifier (agentskills.io spec, e.g. `"MIT"`, `"Apache-2.0"`) |
| `compatibility` | `str` | Environment requirements, max 500 chars (agentskills.io spec) |
| `allowed_tools` | `List[str]` | Pre-approved tools (agentskills.io spec, experimental) |
| `external_deps` | `str \| None` | External dependency declaration as JSON string (MCP servers, env vars, binaries). Set via `md.external_deps = json.dumps(deps)`, read via `json.loads(md.external_deps)`. See [Skill Scopes & Policies](skill-scopes-policies.md) for the full schema. |

## Tool Groups (Progressive Exposure)

Large skills often expose far more tools than an AI client needs at any given
moment. Tool groups let a skill ship several related toolsets and let the
client activate only the ones it needs — keeping `tools/list` small while all
tools remain discoverable.

### Declaring Groups with Sibling Files

Declare groups in `groups.yaml`, referenced from `SKILL.md` via `metadata.dcc-mcp.groups`. Tools then reference a group name through their `group:` field in `tools.yaml`:

```yaml
# SKILL.md frontmatter
---
name: maya-geometry
description: "Domain skill — Maya geometry, modeling, and rigging tools. Use when ..."
metadata:
  dcc-mcp:
    dcc: maya
    tools: tools.yaml
    groups: groups.yaml
---
```

```yaml
# groups.yaml
groups:
  - name: modeling
    description: Polygon modeling and UV tools.
    default_active: true
    tools: [create_sphere, create_cube, extrude]
  - name: rigging
    description: Skeleton, joints, and skinning.
    default_active: false
    tools: [create_joint]
```

```yaml
# tools.yaml
tools:
  - name: create_sphere
    description: Create a polygon sphere.
    group: modeling
    source_file: scripts/create_sphere.py
  - name: create_joint
    description: Create a joint chain.
    group: rigging
    source_file: scripts/create_joint.py
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
3. `SkillCatalog.active_groups()` returns the active group names. `SkillCatalog.list_groups()` returns `(skill_name, group_name, active)` tuples for all declared groups.

### Controlling Groups at Runtime

```python
from dcc_mcp_core import SkillCatalog, ToolRegistry

registry = ToolRegistry()
catalog = SkillCatalog(registry)

# Via the high-level catalog: group names are global registry group keys.
changed = catalog.activate_group("rigging")      # enables rigging tools
changed = catalog.deactivate_group("rigging")    # disables them again
groups = catalog.list_groups()                    # -> [(skill_name, group_name, active)]
active = catalog.active_groups()                  # -> ["modeling", ...]

# Or via the registry directly (the HTTP server emits tools/list_changed notifications).
changed = registry.set_group_enabled("rigging", True)
group_tools = registry.list_actions_in_group("modeling")
enabled_only = registry.list_actions_enabled()
```

### MCP Tools for Group Management

`create_skill_server` / `McpHttpServer` register three core MCP tools for
group control in addition to the six skill-discovery tools:

| Tool | Description |
|------|-------------|
| `activate_tool_group` | Activate a group by `{ "group": "rigging" }`; emits `notifications/tools/list_changed` so clients refresh their view |
| `deactivate_tool_group` | Deactivate a group by `{ "group": "rigging" }` |
| `search_tools` | Keyword search across currently-enabled tools (name, description, tags) |

`tools/list` also returns `__group__<group>` stubs for any group
that is inactive, making the full tool surface discoverable without
exposing schemas or handlers.

### Gateway Progressive Group Search

When skills are exposed through the gateway, the capability index tracks
group activation state. A search hit for a tool in an inactive group
includes:

- `callable: false` — the agent cannot call it yet
- `load_state: "loaded"` — the skill is loaded but the group is inactive
- `disabled_by_group: "modeling"` — which group blocks it
- `next_step` with `action: "activate_tool_group"` — prescriptive agent guidance

The gateway `load_skill` meta-tool supports progressive group activation:

```json
{
  "tool": "load_skill",
  "arguments": {
    "skill_name": "maya-modeling",
    "tool_group": "modeling",
    "group_action": "activate"
  }
}
```

This implements "progressive tool exposure" — tools are discoverable but
not callable until their group is activated, keeping `tools/list` compact
while maintaining full discoverability.

## Skill Persistence Across Restarts

`DccServerBase.enable_skill_load_persistence(policy)` wires the catalog's
after-load / after-unload / after-group-change hooks to a `LoadedStateStore`
backed by `~/.dcc-mcp/<dcc>/loaded.json` (source of truth) and the gateway
admin SQLite `skill_loaded_state` / `skill_active_groups` tables (best-effort
mirror for admin UI visibility).

```python
server = DccServerBase(opts)
server.start()
server.enable_skill_load_persistence(policy="skip_on_drift")
```

On startup the store replays the persisted snapshot so previously loaded
skills and active groups are restored without requiring the AI client to
re-discover and re-activate them.

### Replay policies

| Policy | Behaviour |
|--------|-----------|
| `"skip_on_drift"` (default) | Skip a skill if its on-disk version differs from the persisted version |
| `"require_exact_version"` | Fail the replay if any version mismatch is detected |
| `"ignore_version"` | Always reload regardless of version drift |

### Persistence contract

- Source of truth: per-DCC JSON file at `~/.dcc-mcp/<dcc>/loaded.json`.
- Gateway admin SQLite is a best-effort mirror for dashboard visibility.
- All writes use atomic-replace (`write → fsync → rename`).
- Hooks must never raise; missing files or schema mismatches behave like empty state.

### Key types

```python
from dcc_mcp_core.loaded_state_store import (
    LoadedStateStore,
    LoadedSkillRecord,
    PersistedCatalogState,
)

store = LoadedStateStore("maya")  # path defaults to ~/.dcc-mcp/maya/loaded.json
store.record_loaded("maya-geometry", version="1.0.0", skill_path="/skills/maya-geometry")
store.record_unloaded("maya-geometry")
store.record_group_change("rigging", activated=True)
snapshot = store.snapshot()  # -> PersistedCatalogState
```

## Semantic Skill Search

dcc-mcp-core ships a lexical + vector fusion index so morphology variants
and inflected queries (`rendering` vs `render`) still recall the right skill.

### Standard fusion setup

```python
from dcc_mcp_core import LexicalSkillIndex, RrfFusionIndex, VectorSkillIndex

fused = (
    RrfFusionIndex()
    .register("lex", LexicalSkillIndex())
    .register("vec", VectorSkillIndex())
)
fused.index(documents)
hits = fused.search("how do i create a polygon sphere", k=8)
```

### Components

| Class | Purpose | Dependencies |
|-------|---------|-------------|
| `LexicalSkillIndex` | BM25-style keyword matching | Zero deps (pure Python) |
| `VectorSkillIndex` | Dense embedding similarity search | Zero deps by default (`HashedEmbedder` + `InMemoryVectorStore`) |
| `RrfFusionIndex` | Reciprocal Rank Fusion combiner (Cormack et al. 2009) | Zero deps |
| `HashedEmbedder` | Deterministic feature-hashing embedder (~5 µs/doc at dim=256) | Zero deps |
| `OnnxEmbedder` | High-quality neural embeddings (three-tier backend) | Optional: `dcc-mcp-core-semantic` or `fastembed` |

### Embedder backends

`OnnxEmbedder` uses a three-tier fallback:

1. `dcc_mcp_core_semantic.native.NativeEmbedder` — Rust-native via fastembed-rs (fastest)
2. `fastembed.TextEmbedding` — pure-Python fastembed
3. Neither installed → `EmbedderError`

```python
from dcc_mcp_core import OnnxEmbedder, VectorSkillIndex

emb = OnnxEmbedder(model_name="BAAI/bge-base-en-v1.5")
idx = VectorSkillIndex(embedder=emb)
```

Environment variables for model overrides:

| Variable | Purpose |
|----------|---------|
| `DCC_MCP_EMBED_MODEL` | Override the default ONNX model name |
| `DCC_MCP_EMBED_MODEL_DIR` | Override the local model cache directory |

### `SkillDocument` and `SkillSearchHit`

```python
from dcc_mcp_core.semantic_skill_index import SkillDocument, SkillSearchHit

doc = SkillDocument(
    skill_id="maya-geometry",
    name="maya-geometry",
    summary="Maya geometry creation and modification tools",
    intent="Create spheres, cubes, and extrude polygon components",
    tags=("geometry", "create"),
    dcc_name="maya",
)
# doc.corpus() returns the concatenated text for embedding

hit = SkillSearchHit(skill_id="maya-geometry", score=0.85, rank=1)
```

## Lifecycle Hooks

The typed lifecycle-hook framework lets adapter and policy code subscribe to
discovery, tool-call, and session events without patching `DccServerBase`
internals.

### Hook events

```python
from dcc_mcp_core.lifecycle_hooks import HookEvent

# Available events:
HookEvent.SESSION_START      # on_session_start
HookEvent.BEFORE_SEARCH      # before_search      (policy — can veto)
HookEvent.AFTER_SEARCH        # after_search
HookEvent.BEFORE_SKILL_LOAD   # before_skill_load  (policy — can veto)
HookEvent.AFTER_SKILL_LOAD    # after_skill_load
HookEvent.BEFORE_TOOL_CALL    # before_tool_call   (policy — can veto)
HookEvent.AFTER_TOOL_CALL     # after_tool_call
HookEvent.SESSION_END         # on_session_end
```

Policy events (`BEFORE_SKILL_LOAD`, `BEFORE_TOOL_CALL`, `BEFORE_SEARCH`) can
veto an operation by raising `HookDeny`:

```python
from dcc_mcp_core.lifecycle_hooks import HookDeny

def my_policy(ctx):
    if is_blocked(ctx.payload.get("tool_name")):
        raise HookDeny("Tool blocked by policy", hint="Use dcc_diagnostics__screenshot instead")

hooks.on(HookEvent.BEFORE_TOOL_CALL, my_policy)
```

### Registration

```python
from dcc_mcp_core.lifecycle_hooks import LifecycleHooks, HookEvent

hooks = LifecycleHooks()
server.register_lifecycle_hooks(hooks)

@hooks.on(HookEvent.AFTER_TOOL_CALL)
def log_call(ctx):
    print(f"Tool {ctx.payload.get('tool_name')} called on {ctx.dcc_name}")
```

### Dispatch helpers

`DccServerBase` provides convenience methods for adapter code to fire events:

```python
server.dispatch_session_start(session_id="abc-123")
server.dispatch_before_tool_call("create_sphere", session_id="abc-123")
server.dispatch_after_tool_call("create_sphere", payload={"ok": True})
server.dispatch_session_end(session_id="abc-123")
```

### Fail-safe fan-out

Handlers are dispatched in registration order. For non-policy events, handler
exceptions are logged at WARNING and swallowed. For policy events, a `HookDeny`
raised by any handler propagates to the caller; other exceptions are logged and
treated as "no decision".

## Agent Memory

Three-tier memory model for DCC adapters: bounded, session-aware, and
privacy-conscious. The contract is "summarised facts only, never raw prompts".

### Memory layers

| Layer | Scope | Persistence | Default cap |
|-------|-------|-------------|-------------|
| `EPHEMERAL` | Session-scoped ring-buffer | Never persisted | 256 per session |
| `WORKING` | Task-scoped, TTL-based | Never persisted by default | 1024 per session, 6h TTL |
| `LONGTERM` | Durable patterns | Requires explicit storage backend | 4096 total |

### Quick start

```python
from dcc_mcp_core.agent_memory import InMemoryMemoryStore, MemoryRecorder
from dcc_mcp_core.lifecycle_hooks import LifecycleHooks

hooks = LifecycleHooks()
store = InMemoryMemoryStore()
recorder = MemoryRecorder(store).install(hooks)
server.register_lifecycle_hooks(hooks)
```

`MemoryRecorder.install(hooks)` registers handlers for `SESSION_START`,
`BEFORE_SEARCH`, `AFTER_SKILL_LOAD`, `BEFORE_TOOL_CALL`, `AFTER_TOOL_CALL`,
and `SESSION_END`. The memory summary is automatically injected into
`HookContext.payload` as `memory_summary`, `memory_prefer_tools`, and
`memory_avoid_tools`.

### Session compaction

On `SESSION_END`, working entries are promoted to `LONGTERM` as `"pattern:*"`
summaries, then ephemeral and working entries are forgotten. This keeps the
memory footprint bounded while preserving learned patterns across sessions.

### Privacy toggle

```python
recorder.set_enabled(False)  # disables capture/injection without unregistering hooks
```

### Querying memory

```python
from dcc_mcp_core.agent_memory import MemoryQuery, MemoryLayer

# Query longterm patterns for a specific DCC
results = store.query(MemoryQuery(layer=MemoryLayer.LONGTERM, dcc_name="maya", limit=10))

# Query all layers for a session
results = store.query(MemoryQuery(session_id="abc-123"))

# Forget all entries for a session
forgotten = store.forget(session_id="abc-123")
```

## Capability Graph (#1336)

The `CapabilityGraph` models directed relationships between skills, tools, and
named capabilities so the search ranker, agent planner, and admin UI can answer:

- "What else do I need before this skill is useful?" (`REQUIRES`)
- "What does this skill produce that other skills need?" (`PRODUCES`)
- "If this skill is missing, what is the fallback?" (`FALLBACK_FOR`)

### Edge kinds

| Kind | Direction | Meaning |
|------|-----------|---------|
| `DEPENDS_ON` | source → target | Source depends on target (weaker than `REQUIRES`) |
| `REQUIRES` | source → target | Source cannot function without target |
| `PRODUCES` | source → target | Source generates capability that target consumes |
| `USED_IN` | source → target | Source capability is used in target context |
| `COMPATIBLE_WITH` | source ↔ target | Bidirectional compatibility (soft) |
| `REPLACES` | source → target | Source supersedes target |
| `FALLBACK_FOR` | source → target | Source is a fallback when target is unavailable |

### Quick start

```python
from dcc_mcp_core import CapabilityGraph, CapabilityEdge, EdgeKind

graph = CapabilityGraph()

# Register a skill with its declared metadata
graph.register_skill(
    "maya-geometry",
    requires=["dcc-diagnostics"],
    produces=["geometry-output"],
    depends_on=["maya-core"],
    compatible_with=["maya-render"],
)

# Add individual edges
graph.add_edge(CapabilityEdge(
    source="maya-geometry",
    target="maya-modeling",
    kind=EdgeKind.COMPATIBLE_WITH,
    weight=0.8,
))
```

### Expansion and query

```python
# Bounded BFS expansion (depth clamped to [1, 16])
reachable = graph.expand(["maya-geometry"], max_depth=2, direction="out")

# Inspect neighbors
deps = graph.neighbors("maya-geometry", kinds=[EdgeKind.REQUIRES], direction="out")

# Serialize
payload = graph.to_json()  # {"nodes": [...], "edges": [...]}
restored = CapabilityGraph.from_json(payload)

# Stats
print(len(graph))          # node count
print(graph.edge_count())  # edge count
```

The graph is **threadsafe** (RLock) and designed as pure data with bounded
expansion — no I/O, no async, no embedding inference. Idempotent inserts
prevent duplicate edges on re-registration.

### Integration points

The capability graph feeds into:
- **Semantic skill index** (#1333) — enrichment of search hits with graph context
- **Agent memory layers** (#1334) — pattern-level recall of successful skill chains
- **Gateway search reranker** — boosting results that have verified `REQUIRES` edges satisfied by loaded skills

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

## Built-in Skills Unified Registration (#1332)

`register_all_builtin_skills()` registers the full set of standard, host-local
tools in one idempotent call. Every DCC adapter gets diagnostics, introspection,
feedback, recipes, Qt UI inspection, and script materialization without wiring
each family by hand.

```python
from dcc_mcp_core import DccServerBase, DccServerOptions
from dcc_mcp_core.skills.builtin import register_all_builtin_skills

opts = DccServerOptions.from_env("maya")
server = DccServerBase(opts)
register_all_builtin_skills(
    server,
    dcc_name="maya",
    dcc_pid=os.getpid(),
    dcc_window_title="Autodesk Maya",
)
server.start()
```

### Registered tool families

| Family | Prefix | Tools | Description |
|--------|--------|-------|-------------|
| Diagnostics | `dcc_diagnostics__*` | `screenshot`, `audit_log`, `tool_metrics`, `process_status`, `gateway_failover` | Host-local diagnostics and observability |
| Introspection | `dcc_introspect__*` | `list_module`, `signature`, `search`, `eval` | Runtime DCC namespace introspection |
| Feedback | `dcc_feedback__*` | `report` | Agent feedback / rationale capture |
| Recipes | `dcc_recipes__*` | `list`, `search`, `get`, `validate`, `apply` | Skill/domain recipe operations |
| Qt Inspector | `qt_ui_inspector__*` | `list_windows`, `find_widgets`, `describe_widget`, `snapshot_tree`, `wait_for_widget` | DCC-agnostic Qt widget introspection |
| Script Materialization | `materialize__*` | `materialize_script` | Host-local script file materialization |

The call is **idempotent** — adapters can call it multiple times (e.g. once at
base-server init with an empty skill set, then again after scanning skills to
populate recipe data). The `skills` parameter forwards `SkillMetadata` objects
to `register_recipes_tools`; pass `None` (the default) when skills haven't been
scanned yet.

`DccServerBase.__init__` calls `register_all_builtin_skills` automatically, so
most adapters don't need to call it directly. Use the standalone function only
when building a custom server that does not inherit from `DccServerBase`.

## Qt UI Inspector (#1332)

The Qt UI inspector is a **DCC-agnostic, read-only** capability that lets AI
agents inspect the Qt widget tree of any Qt-based DCC host (Maya, Houdini, Nuke,
Substance, Katana, etc.). It imports the Qt binding lazily so the module never
pulls Qt into the host on import.

### Supported Qt bindings (priority order)

1. `qtpy` — abstraction layer (recommended)
2. `PySide6` — Qt 6 official bindings
3. `PySide2` — Qt 5 official bindings
4. `PyQt6` — Qt 6 Riverbank bindings
5. `PyQt5` — Qt 5 Riverbank bindings

When no binding is importable, every tool returns a structured
`qt-binding-unavailable` envelope rather than crashing the host.

### Tools

| Tool | Description | Key Parameters |
|------|-------------|----------------|
| `qt_ui_inspector__list_windows` | List every top-level Qt window with object name, class, visibility, geometry, and child count | `include_hidden`, `max_results` (max 256) |
| `qt_ui_inspector__find_widgets` | Locate Qt widgets by object name (exact/substring/regex), class name, and visibility | `object_name`, `class_name`, `visible_only`, `max_results` |
| `qt_ui_inspector__describe_widget` | Return a single widget's structured state: class, geometry, flags, accessible name/description, bounded property snapshot (≤32 properties) | `widget_id` (required) |
| `qt_ui_inspector__snapshot_tree` | Walk the Qt widget tree from a root and return a JSON-safe tree with depth (≤16) and node-count (≤4096) budgets | `root_widget_id`, `max_depth`, `max_nodes` |
| `qt_ui_inspector__wait_for_widget` | Poll for a widget by name/class with visible/enabled gates and bounded timeout (≤60 s) | `object_name`, `class_name`, `visible`, `enabled`, `timeout_ms` |

### One-line registration

```python
from dcc_mcp_core import register_qt_ui_inspector
register_qt_ui_inspector(server, dcc_name="maya")
```

Call this **before** `server.start()`. `register_all_builtin_skills` calls it
automatically, so adapters using that path don't need a separate call.

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

Searches across: `name`, `description`, `search_hint`, `search_aliases`,
`tags`, `dcc`, and the sibling `tools.yaml` entries (`name`, `description`,
and per-tool `search_aliases`). The `search_hint` field (from
`metadata.dcc-mcp.search-hint`) improves keyword matching without loading full
schemas.

#### How `search_skills` ranks results

Since dcc-mcp-core 0.15 (issue [#343](https://github.com/dcc-mcp/dcc-mcp-core/issues/343))
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
| `search_aliases`                         | 3.0 |
| `description`                            | 2.0 |
| sibling tool names (from `tools.yaml`)   | 2.0 |
| sibling tool search aliases              | 2.0 |
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

Starting with dcc-mcp-core 0.15 (issue [#356](https://github.com/dcc-mcp/dcc-mcp-core/issues/356)), dcc-mcp-core-specific extension keys (`dcc`, `version`, `tags`, `tools`, …) MUST live under the agentskills.io-compliant nested `metadata.dcc-mcp` namespace rather than at the top level of SKILL.md frontmatter. The strict v0.15+ loader also no longer promotes the pre-0.15 flat dotted form (`metadata: { "dcc-mcp.dcc": ... }`) into typed fields. A SKILL.md with legacy top-level keys fails to load and emits a `tracing::error!`.

### Before (pre-0.15 legacy form — no longer accepted)

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
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    tags: [geometry, create]
    search-hint: "polygon modeling, sphere, bevel, extrude"
    tools: tools.yaml     # sibling file (relative to SKILL.md)
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

A top-level `next-tools:` block on SKILL.md is rejected by the loader
(issue #356 strict mode) — move per-tool `next-tools` entries into
`tools.yaml` alongside the tool they belong to.

### Metadata key reference

| Legacy top-level (no longer accepted) | Spec-compliant `metadata` key                | Value type            |
| ------------------------------------- | -------------------------------------------- | --------------------- |
| `dcc: maya`                           | `metadata["dcc-mcp.dcc"]`                    | string                |
| `version: 1.0.0`                      | `metadata["dcc-mcp.version"]`                | string                |
| `tags: [a, b]`                        | `metadata["dcc-mcp.tags"]`                   | comma-separated string |
| `search-hint: "…"`                    | `metadata["dcc-mcp.search-hint"]`            | string                |
| `depends: [x, y]`                     | `metadata["dcc-mcp.depends"]`                | list or comma-separated string |
| `products: [maya]`                    | `metadata["dcc-mcp.products"]`               | comma-separated string |
| `allow_implicit_invocation`           | `metadata["dcc-mcp.allow-implicit-invocation"]` | `"true"` / `"false"` |
| `external_deps: {...}`                | `metadata["dcc-mcp.external-deps"]`          | JSON string           |
| `tools: [...]` inline block           | `metadata["dcc-mcp.tools"]`                  | sibling `.yaml` file  |
| `groups: [...]` inline block          | `metadata["dcc-mcp.groups"]`                 | sibling `.yaml` file  |
| *(MCP prompts primitive)*             | `metadata["dcc-mcp.prompts"]`                | sibling `.yaml` file or `prompts/*.prompt.yaml` glob |
| *(MCP resources primitive)*           | `metadata["dcc-mcp.resources"]`              | sibling `.yaml` file, directory, or `resources/*.resource.yaml` glob |

### Validating your skill

`validate_skill()` reports any non-spec top-level key as a frontmatter
error. Use it from a pre-commit hook or CI to catch migrations that
were missed:

```python
report = dcc_mcp_core.validate_skill("/path/to/my-skill")
if report.has_errors:
    for issue in report.issues:
        print(f"[{issue.severity}] {issue.category}: {issue.message}")
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
| Static MCP resources | `metadata["dcc-mcp.resources"]` | `resources/*.resource.yaml` | #733 |
| Example dialogues | `metadata["dcc-mcp.examples"]` | `references/EXAMPLES.md` or `examples/*.md` | #1236 |
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
├── resources/               # many files — one per MCP resource bundle
│   ├── cmds_help.resource.yaml
│   └── help/
│       └── polySphere.txt
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

### Declaring static MCP resources

A skill can expose static reference material through MCP `resources/list`
and `resources/read` without Python glue. Point `metadata.dcc-mcp.resources`
at a resources directory, glob, or single YAML file:

```yaml
metadata:
  dcc-mcp:
    resources: resources/
```

Each `.resource.yaml` file may contain one resource, a top-level list, or a
`resources:` list. Static file resources use paths relative to the YAML file:

```yaml
resources:
  - uri: maya-cmds://help/polySphere
    name: cmds.polySphere help
    mimeType: text/plain
    description: Output of cmds.help('polySphere') captured at skill build time.
    source:
      type: file
      path: help/polySphere.txt
```

Loaded skills contribute these entries to `resources/list`; `resources/read`
returns text for `text/*`, JSON, YAML, XML, and JavaScript MIME types, and a
base64 `blob` for other MIME types.

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

### Using the `dcc-mcp-skills-creator` Skill

The `skills/dcc-mcp-skills-creator/` skill provides scaffolding and validation
helpers as MCP tools, plus the agent-facing authoring guidance for building or
modernizing DCC-MCP adapter skill packages:

```python
# Scaffold a new skill directory via the loaded MCP tool:
# dcc_mcp_skills_creator__create_skill(name="my-new-skill", parent_dir="/path/to/skills", dcc="maya")
# Creates: my-new-skill/SKILL.md, tools.yaml, scripts/, metadata/

# Get a current SKILL.md template via:
# dcc_mcp_skills_creator__skill_template()
```

Load it before changing `SKILL.md`, `tools.yaml`, adapter skill taxonomies, or
host-specific scripts in repositories such as `dcc-mcp-maya`,
`dcc-mcp-blender`, `dcc-mcp-3dsmax`, or future DCC adapters. Its `references/`
cover:

- `SKILL.md`, `tools.yaml`, and script authoring checklists.
- DCC tool contracts and host differences across Maya, Blender, 3ds Max,
  Houdini, bridge-based hosts, and custom studio tools.
- Unit, lint, gateway, E2E, and VRS validation choices.
