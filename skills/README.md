# dcc-mcp-core Skills Templates

Starter templates for creating new MCP skills. Copy a template directory,
customise the `SKILL.md` frontmatter and scripts, then add the parent path to
`DCC_MCP_SKILL_PATHS` so the gateway discovers your skill automatically.

## Quick Start

```bash
# 1. Copy a template
cp -r skills/templates/minimal my-skills/my-new-skill

# 2. Edit SKILL.md (name, description, dcc, tags, tools)
$EDITOR my-skills/my-new-skill/SKILL.md

# 3. Write your script(s) in scripts/
$EDITOR my-skills/my-new-skill/scripts/hello.py

# 4. Register the path so the gateway discovers it
export DCC_MCP_SKILL_PATHS="/path/to/my-skills"

# 5. Start the MCP server — your skill appears as a stub in tools/list
python -c "
from dcc_mcp_core import create_skill_server, McpHttpConfig
server = create_skill_server('maya', McpHttpConfig(port=8765))
handle = server.start()
print(handle.mcp_url())
input('Press Enter to stop...')
handle.shutdown()
"
```

## Templates

| Template | Use Case | Features |
|----------|----------|----------|
| [`minimal`](templates/minimal/) | Simplest possible skill | 1 tool, 1 script, no groups |
| [`dcc-specific`](templates/dcc-specific/) | DCC-bound skill (Maya, Blender, etc.) | `dcc:` field, `required_capabilities`, `next-tools` |
| [`with-groups`](templates/with-groups/) | Progressive exposure via tool groups | `groups:` field, `default-active` toggle |

## Skill Anatomy

```
my-skill/
  SKILL.md          # Frontmatter (name, dcc, tags, tools) + body docs
  scripts/           # One file per tool (*.py, *.sh, *.bat, *.js, *.ts)
    tool_a.py
    tool_b.sh
  metadata/          # Optional: help.md, install.md, depends.md
```

### SKILL.md Frontmatter Fields

| Field | Required | Description |
|-------|----------|-------------|
| `name` | Yes | Unique skill identifier (kebab-case) |
| `description` | Yes | What the skill does (shown to AI agents) |
| `dcc` | No | Target DCC (`maya`, `blender`, `python`, etc.) |
| `version` | No | Semantic version (default `1.0.0`) |
| `tags` | No | Discovery tags (`[modeling, geometry, maya]`) |
| `search-hint` | No | Extra keywords for `search_skills()` matching |
| `license` | No | License identifier (default MIT) |
| `depends` | No | List of skill names this skill requires |
| `groups` | No | Tool groups for progressive exposure |
| `tools` | No | Explicit tool declarations with schemas |
| `metadata` | No | Arbitrary key-value metadata |

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
    next-tools:                  # Suggested follow-up tools
      on-success: [other_skill__tool]
      on-failure: [dcc_diagnostics__screenshot]
```

## Existing Examples

See [`examples-index.md`](examples-index.md) for a full index of the 11 example
skills shipped in `examples/skills/`, or browse them directly:

| Skill | DCC | Category | Key Feature |
|-------|-----|----------|-------------|
| [hello-world](../examples/skills/hello-world/) | python | example | Minimal starter |
| [maya-geometry](../examples/skills/maya-geometry/) | maya | modeling | Tool groups |
| [maya-pipeline](../examples/skills/maya-pipeline/) | maya | pipeline | Dependencies + metadata/ |
| [git-automation](../examples/skills/git-automation/) | python | devops | OpenClaw format |
| [ffmpeg-media](../examples/skills/ffmpeg-media/) | python | media | External binary deps |
| [imagemagick-tools](../examples/skills/imagemagick-tools/) | python | image | OpenClaw install |
| [usd-tools](../examples/skills/usd-tools/) | python | pipeline | Read-only tools |
| [multi-script](../examples/skills/multi-script/) | python | example | .py + .sh + .bat |
| [clawhub-compat](../examples/skills/clawhub-compat/) | python | example | Full OpenClaw |
| [dcc-diagnostics](../examples/skills/dcc-diagnostics/) | python | diagnostics | Also bundled |
| [workflow](../examples/skills/workflow/) | python | workflow | Also bundled |

## Bundled Skills

Two skills ship inside the `dcc-mcp-core` wheel and are available immediately
after `pip install dcc-mcp-core` (no `DCC_MCP_SKILL_PATHS` needed):

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
