---
name: dcc-mcp-skill-developer
description: >-
  Infrastructure skill - guide agents through designing, implementing, testing,
  and reviewing DCC-MCP adapter skill packages for Maya, Blender, 3ds Max,
  Houdini, Photoshop, ZBrush, Unreal, Unity, and custom studio hosts. Use when
  adding or changing SKILL.md, tools.yaml, scripts, server wiring, or adapter
  skill taxonomy in dcc-mcp-* repositories. Not for driving a live DCC scene -
  use domain skills or dcc-cli-gateway for that.
license: MIT
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write Edit
metadata:
  dcc-mcp:
    dcc: python
    layer: infrastructure
    version: "1.0.0"
    search-hint: >-
      develop dcc-mcp skill, adapter skill authoring, tools.yaml, SKILL.md,
      Maya Blender 3ds Max, affinity, execution, stage taxonomy, gateway
    tags: "skill-authoring, adapter, maya, blender, 3dsmax, future-dcc"
    skill-reference-docs:
      - "references/*.md"
---

# DCC-MCP Skill Developer

Use this skill when you are changing or creating DCC-MCP adapter skill packages.
It distills patterns from dcc-mcp-maya, dcc-mcp-blender, and dcc-mcp-3dsmax
into a faster authoring loop.

## Fast Workflow

1. Classify the work: new host adapter, new domain skill, new infrastructure
   skill, or porting an existing skill to another DCC.
2. Read only the reference you need:
   - [ADAPTER_PATTERNS.md](references/ADAPTER_PATTERNS.md) for server and
     composition-root patterns.
   - [SKILL_AUTHORING_CHECKLIST.md](references/SKILL_AUTHORING_CHECKLIST.md)
     for SKILL.md, tools.yaml, and scripts.
   - [HOST_MATRIX.md](references/HOST_MATRIX.md) for Maya, Blender, 3ds Max,
     and future DCC differences.
   - [TESTING_MATRIX.md](references/TESTING_MATRIX.md) for unit, lint, gateway,
     E2E, and VRS coverage.
3. Prefer existing adapter helpers before adding new abstractions.
4. Keep DCC identity parameterized: `dcc_name`, `dcc_type`, environment prefixes,
   skill names, and search examples.
5. Make every tool declaration explicit: `source_file`, `execution`, `affinity`,
   safety annotations, and `timeout_hint_secs` for async tools.
6. When changing adapter server wiring or caller examples, keep Admin telemetry
   useful: pass optional `agent_context` / `caller_context` summaries through
   MCP `_meta`, REST `meta`, or `x-dcc-mcp-agent-*` headers when the caller is
   an agent. Include only explicit summaries, plans, observations, and
   correlation ids; never ask tools to expose hidden chain-of-thought. Preserve
   Admin `links` fields in examples so every trace/debug bundle, OpenAPI
   Inspector/spec link, or issue-report JSON export can be copied as a complete
   URL into a follow-up agent, LLM evaluation prompt, or GitHub issue.
7. Add tests at the lowest executable layer, then one discovery/load/call or
   gateway REST path when behavior crosses MCP or REST boundaries.

## Adapter Selection

- Use Maya patterns for mature stage taxonomy, main-thread dispatch,
  cancellation, resources, readiness, capability manifests, and strict skill
  linting.
- Use Blender patterns for a lean `DccServerBase` adapter scaffold and
  progressive loading helpers.
- Treat current 3ds Max skills as migration targets: preserve the pymxs domain
  logic, but modernize into nested `SKILL.md`, `tools.yaml`, and scripts.
- For future hosts, start from Blender's lean scaffold, then add Maya-style
  lifecycle hardening only when the host actually needs it.

## Non-Negotiables

- No top-level dcc-mcp extension keys in SKILL.md.
- No host API imports at module import time in skill scripts.
- No scene-touching tool without `affinity: main`.
- No `execution: async` without a realistic `timeout_hint_secs`.
- No new generic helper crate or module when core or an adapter-local owner
  already exists.
- No raw `execute_python` or `execute_mel` as the primary UX when a typed tool
  can exist.
