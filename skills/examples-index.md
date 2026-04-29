# Existing Skill Examples Index

Reference implementations shipped in [`examples/skills/`](../examples/skills/).
Each demonstrates a specific skill system feature.

## Skill Matrix

| Skill | DCC | Category | Tools | Key Feature |
|-------|-----|----------|-------|-------------|
| **hello-world** | python | example | greet | Minimal starter — 1 tool, 1 script |
| **maya-geometry** | maya | modeling | create_sphere, bevel_edges, create_joint | **Tool groups** (modeling=active, rigging=inactive) |
| **maya-pipeline** | maya | pipeline | setup_project, export_usd | **Dependencies** (requires maya-geometry + usd-tools), metadata/ dir |
| **git-automation** | python | devops | log, diff | OpenClaw format with binary requirements |
| **ffmpeg-media** | python | media | convert, extract_frames | External binary deps + OpenClaw install instructions |
| **imagemagick-tools** | python | image | resize, composite | OpenClaw format with enum input constraints |
| **usd-tools** | python | pipeline | inspect, validate | Read-only tools, Apache-2.0 license |
| **multi-script** | python | example | action_python, action_shell, action_batch | **Cross-platform**: .py + .sh + .bat in one skill |
| **clawhub-compat** | python | example | (scripts only) | Full **OpenClaw/ClawHub** compatibility reference |
| **dcc-diagnostics** | python | diagnostics | screenshot, audit_log, tool_metrics, process_status | **Also bundled** in wheel |
| **workflow** | python | workflow | run_chain | **Also bundled** in wheel |
| **example-layered-skill** | python | example | create_asset, publish_asset, validate_asset | **Layered architecture** — Tools / Services / Utils internal split (issue #575) |

## By Feature

### Tool Groups (Progressive Exposure)
- **maya-geometry** — `modeling` (default active) + `rigging` (activate on demand)

### Skill Dependencies
- **maya-pipeline** — `depends: [maya-geometry, usd-tools]`

### OpenClaw / ClawHub Compatibility
- **clawhub-compat** — Full format reference (env vars, bins, Node packages)
- **git-automation** — `openclaw.requires.bins: [git]`
- **ffmpeg-media** — `openclaw.install: [{kind: brew, formula: ffmpeg}]`
- **imagemagick-tools** — `openclaw.install: [{kind: brew, formula: imagemagick}]`

### Cross-Platform Scripts
- **multi-script** — Python + Shell + Batch in one skill

### Internal Layered Architecture (Tools / Services / Utils)
- **example-layered-skill** — reference layout for complex skills with shared
  business logic; see `docs/guide/skills.md` "Complex Skill Architecture"

### Next-Tools Chaining
- **maya-geometry** — `on-success: [maya_pipeline__export_usd]`, `on-failure: [dcc_diagnostics__screenshot]`
- **maya-pipeline** — `on-success: [usd_tools__inspect]`
- **dcc-diagnostics** — `on-failure: [dcc_diagnostics__screenshot]`

> `next-tools` is a **dcc-mcp-core extension** (not in agentskills.io spec). It guides AI agents
> to follow-up tools via `on-success` and `on-failure` keys.

### Metadata Directory
- **maya-pipeline** — `metadata/help.md`, `metadata/install.md`, `metadata/depends.md`

## Bundled Skills

Two skills are **also shipped inside the wheel** (available without `DCC_MCP_SKILL_PATHS`):
- `dcc-diagnostics` — Same as `examples/skills/dcc-diagnostics/`
- `workflow` — Same as `examples/skills/workflow/`

Access via:
```python
from dcc_mcp_core import get_bundled_skill_paths
paths = get_bundled_skill_paths()  # [".../dcc_mcp_core/skills"]
```
