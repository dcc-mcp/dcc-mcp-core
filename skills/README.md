# dcc-mcp-core Skills

Official skills and starter templates for the dcc-mcp-core ecosystem.

## Bundled Skills

| Skill | Description | Location |
|-------|-------------|----------|
| [`dcc-skills-creator`](dcc-skills-creator/) | Create, validate, and scaffold DCC skills | `skills/dcc-skills-creator/` |

## Quick Start

### Using the dcc-skills-creator

```bash
# Add the skills directory to your path
export DCC_MCP_SKILL_PATHS="/path/to/dcc-mcp-core/skills"

# Start the MCP server — dcc-skills-creator appears in tools/list
python -c "
from dcc_mcp_core import create_skill_server, McpHttpConfig
server = create_skill_server('maya', McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())
input('Press Enter to stop...')
handle.shutdown()
"
```

Then use the skill's tools:
- `create_skill` — scaffold a new skill directory
- `validate_skill_dir` — validate a skill against the spec
- `skill_template` — get a full SKILL.md template

### Manual Template Copy

```bash
# 1. Copy a template
cp -r skills/templates/minimal my-skills/my-new-skill

# 2. Edit SKILL.md (name, description, dcc, tags, tools)
$EDITOR my-skills/my-new-skill/SKILL.md

# 3. Write your script(s) in scripts/
$EDITOR my-skills/my-new-skill/scripts/hello.py

# 4. Validate before loading
python -c "from dcc_mcp_core import validate_skill; print(validate_skill('my-skills/my-new-skill').is_clean)"

# 5. Register the path so the gateway discovers it
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"
```

## Templates

| Template | Use Case | Features |
|----------|----------|----------|
| [`minimal`](templates/minimal/) | Simplest possible skill | 1 tool, 1 script, no groups |
| [`dcc-specific`](templates/dcc-specific/) | DCC-bound skill (Maya, Blender, etc.) | `dcc:` field, `required_capabilities`, `next-tools` |
| [`with-groups`](templates/with-groups/) | Progressive exposure via tool groups | `groups:` field, `default-active` toggle |
| [`domain-skill`](templates/domain-skill/) | Business workflow skill with layering | `dcc-mcp.layer: domain`, negative routing, `depends:`, failure chains |
| [`thin-harness`](templates/thin-harness/) | Raw script execution + recipe book (no wrappers) | `dcc-mcp.layer: thin-harness`, `recipes:`, `introspection:`, `execute_python` |

## Skill Layering

Every skill must belong to one of four layers. Set the layer in `metadata`:

```yaml
metadata:
  dcc-mcp.layer: infrastructure   # low-level reusable primitive
  # dcc-mcp.layer: domain         # business workflow, depends on infrastructure
  # dcc-mcp.layer: thin-harness   # raw script execution + recipes (fall-through)
  # dcc-mcp.layer: example        # authoring reference, never used in production
```

### Layer definitions

| Layer | Role | Examples |
|-------|------|---------|
| **infrastructure** | Low-level, DCC-agnostic primitives. No business context. Stable API. Auto-loaded or shared across all servers. | `dcc-diagnostics`, `workflow`, `usd-tools`, `ffmpeg-media`, `imagemagick-tools`, `git-automation` |
| **domain** | Business workflows for a specific DCC or task area. Depends on infrastructure skills. Loaded on-demand per DCC. | `maya-geometry`, `maya-pipeline`, `maya-animation`, `blender-rigging` |
| **thin-harness** | Raw script execution + recipe book. Primary fall-through when no domain skill matches. One `execute_python` tool + `references/RECIPES.md`. See [thin-harness guide](../docs/guide/thin-harness.md). | `maya-scripting`, `blender-scripting`, `houdini-scripting` |
| **example** | Authoring references and demos only. Never loaded in production environments. | `hello-world`, `multi-script`, `async-render-example`, `cancellable-loop` |

### Description pattern (required for all skills)

Every skill `description` must follow this 3-part structure (max 1024 chars total):

```
<Layer> skill — <one-sentence what + scope keywords>. Use when <trigger>.
Not for <counter-example> — use <other-skill> for that.
```

**Infrastructure example:**
```yaml
description: >-
  Infrastructure skill — low-level OpenUSD scene inspection and validation:
  read layer stacks, traverse prims, validate schemas. Use when working
  directly with raw USD files. Not for Maya-specific USD export — use
  maya-pipeline__export_usd for that.
```

**Domain example:**
```yaml
description: >-
  Domain skill — Maya geometry primitives: create spheres, cubes, cylinders;
  bevel and extrude polygon components. Use for individual geometry operations
  in Maya. Not for full asset export pipelines — use maya-pipeline for that.
  Not for raw USD file inspection — use usd-tools for that.
```

**Example/demo:**
```yaml
description: >-
  Example skill — demonstrates <feature>. Use as a reference when authoring
  new skills. Not intended for production use.
```

### search-hint partitioning

Keep `search-hint` keywords **non-overlapping** across layers so `search_skills()`
returns the most relevant skill without ambiguity:

- **Infrastructure**: mechanism-oriented — describe the underlying tool/API
  (`"usd stage, prim, schema validation, usdcat"`)
- **Domain**: intent-oriented — describe the user's goal
  (`"export Maya scene to USD, asset pipeline, project setup"`)
- **Example**: append `"authoring reference"` to prevent accidental production matches

### next-tools failure chains

Every domain skill tool **must** include `on-failure` pointing to infrastructure diagnostics:

```yaml
next-tools:
  on-success: [next_logical_tool]
  on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

Infrastructure skills do not require `on-failure` chains (they ARE the fallback).

### depends declaration

Domain skills **must** declare `depends:` for every infrastructure skill they chain to via
`next-tools.on-failure`. This ensures the infrastructure skill is loaded before the domain
skill is activated:

```yaml
depends:
  - dcc-diagnostics   # required for on-failure chains
  - usd-tools         # required if any tool exports/validates USD
```

## Skill Anatomy

```
my-skill/
  SKILL.md          # Frontmatter (name, dcc, metadata, tools) + body docs
  scripts/           # One file per tool (*.py, *.sh, *.bat, *.js, *.ts)
    tool_a.py
    tool_b.sh
  references/        # Optional: per-topic knowledge loaded on demand
    guide.md
```

### SKILL.md Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique skill identifier (kebab-case, max 64 chars) |
| `description` | Yes | What the skill does AND when to use it (shown to AI agents, max 1024 chars). Include specific keywords for discoverability. |
| `license` | No | License identifier (agentskills.io spec, e.g. `MIT`, `Apache-2.0`) |
| `compatibility` | No | Environment requirements, max 500 chars (agentskills.io spec, e.g. `"Maya 2024+, Python 3.7+"`) |
| `allowed-tools` | No | Pre-approved tools, space-separated (agentskills.io spec, **experimental**, e.g. `Bash(git:*) Read`) |
| `depends` | No | List of skill names this skill requires |
| `metadata` | No | Namespaced key-value metadata (agentskills.io spec). Use `dcc-mcp.*` keys for dcc-mcp-core extensions. |
| `metadata.dcc-mcp.dcc` | No | Target DCC (`maya`, `blender`, `python`, etc.) |
| `metadata.dcc-mcp.version` | No | Semantic version (default `1.0.0`) |
| `metadata.dcc-mcp.layer` | **Yes** | Skill layer: `infrastructure`, `domain`, or `example` |
| `metadata.dcc-mcp.search-hint` | No | Extra keywords for `search_skills()` matching |
| `metadata.dcc-mcp.tags` | No | Comma-separated discovery tags |
| `metadata.dcc-mcp.tools` | No | Sibling file path for tool declarations (e.g. `tools.yaml`) |

### Description Quality Guide

The `description` field is the **most important field for AI agent discoverability**. It
determines whether an agent will find and use your skill via `search_skills()`.

**Good descriptions** tell AI agents **what**, **when to use**, and **when NOT to use**:
```yaml
# ✓ Infrastructure — mechanism + negative routing
description: >-
  Infrastructure skill — raw USD file inspection via usdcat/usdchecker.
  Use when validating or reading a .usd file directly. Not for Maya USD
  export — use maya-pipeline__export_usd for that.

# ✓ Domain — intent + explicit scope + counter-examples
description: >-
  Domain skill — Maya polygon geometry: create, bevel, extrude. Use when
  the user asks to build or modify 3D meshes in Maya. Not for export
  pipelines — use maya-pipeline for that.
```

**Bad descriptions** are vague, lack layer prefix, or have no counter-examples:
```yaml
# ✗ Bad — no layer, no trigger, no counter-examples
description: "Helps with geometry."
# ✗ Bad — no when-to-use, no negative routing
description: "USD processing utilities."
```

### Progressive Disclosure

Keep `SKILL.md` body under **500 lines / 5000 tokens**. Move detailed references to `references/`:
- AI agents load `name` + `description` at startup (~100 tokens)
- Full SKILL.md body is loaded on `load_skill()` activation
- `references/` files are loaded only when explicitly needed

### Tool Declaration Fields

```yaml
tools:
  - name: my_tool               # Required: tool name (snake_case)
    description: "What it does"  # Required: shown to AI agents
    input_schema:                # Optional: JSON Schema for parameters
      type: object
      properties:
        param1: { type: string, description: "..." }
    read_only: true              # Hint: does not modify state
    destructive: false           # Hint: cannot be undone
    idempotent: true             # Hint: safe to call multiple times
    source_file: scripts/my_tool.py  # Script file path
    group: basic                 # Tool group name (if using groups)
    next-tools:                  # dcc-mcp-core extension: follow-up tools
      on-success: [other_skill__tool]
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
```

**`next-tools`** is a dcc-mcp-core extension (not in agentskills.io spec). It guides AI agents
to chain tool calls:
- `on-success`: suggested tools after successful execution
- `on-failure`: debugging/recovery tools on failure — **always point to `dcc_diagnostics__*`**
- Both accept lists of fully-qualified tool names (`skill_name__tool_name` format)

## Existing Examples

See [`examples-index.md`](examples-index.md) for a full index of the 11 example
skills shipped in `examples/skills/`, or browse them directly:

| Skill | Layer | DCC | Key Feature |
|-------|-------|-----|-------------|
| [hello-world](../examples/skills/hello-world/) | example | python | Minimal starter |
| [multi-script](../examples/skills/multi-script/) | example | python | .py + .sh + .bat |
| [async-render-example](../examples/skills/async-render-example/) | example | python | Async/deferred tools |
| [cancellable-loop](../examples/skills/cancellable-loop/) | example | python | Cooperative cancellation |
| [clawhub-compat](../examples/skills/clawhub-compat/) | example | python | Full OpenClaw format |
| [dcc-diagnostics](../examples/skills/dcc-diagnostics/) | infrastructure | python | Also bundled |
| [workflow](../examples/skills/workflow/) | infrastructure | python | Also bundled |
| [usd-tools](../examples/skills/usd-tools/) | infrastructure | python | Read-only USD tools |
| [ffmpeg-media](../examples/skills/ffmpeg-media/) | infrastructure | python | External binary deps |
| [imagemagick-tools](../examples/skills/imagemagick-tools/) | infrastructure | python | OpenClaw install |
| [git-automation](../examples/skills/git-automation/) | infrastructure | python | OpenClaw format |
| [maya-geometry](../examples/skills/maya-geometry/) | domain | maya | Tool groups |
| [maya-pipeline](../examples/skills/maya-pipeline/) | domain | maya | Dependencies + metadata/ |

## Bundled Skills

Two skills ship inside the `dcc-mcp-core` wheel and are available immediately
after `pip install dcc-mcp-core` (no `DCC_MCP_SKILL_PATHS` needed).
Both are **infrastructure** skills:

- **dcc-diagnostics** — screenshot, audit_log, tool_metrics, process_status
- **workflow** — run_chain (multi-step orchestration)

```python
from dcc_mcp_core import get_bundled_skill_paths
paths = get_bundled_skill_paths()  # [".../dcc_mcp_core/skills"]
```

## DCC Integration Guide

Building a new MCP adapter for a DCC application? See the
**[Integration Guide](integration-guide.md)** for complete architecture patterns:

| Architecture | For | Base Class | Examples |
|---|---|---|---|
| **A: Embedded Python** | DCCs with built-in Python | `DccServerBase` | Maya, Blender, Houdini, Unreal |
| **B: WebSocket Bridge** | DCCs without Python | `DccServerBase` + `DccBridge` | Photoshop, ZBrush, Unity |
| **C: WebView Host** | Browser panels inside DCCs | `WebViewAdapter` | AuroraView, Electron |
