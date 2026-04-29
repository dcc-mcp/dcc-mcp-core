---
name: example-layered-skill
description: >-
  Example skill — reference implementation of the **internal** layered
  architecture pattern (Tools / Services / Utils) for complex skills with
  shared business logic. Use as a template when a skill outgrows a single
  scripts/execute.py file. Not intended for production use — see
  docs/guide/skills.md for the architectural guide.
license: MIT
compatibility: Python 3.8+
allowed-tools: Bash Read
metadata:
  dcc-mcp.dcc: python
  dcc-mcp.version: "1.0.0"
  dcc-mcp.layer: example
  dcc-mcp.search-hint: "layered architecture, complex skill, services, utils, tools, asset pipeline reference, authoring reference"
  dcc-mcp.tags: "example, architecture, layered, advanced, authoring reference"
  dcc-mcp.tools: tools.yaml
  dcc-mcp.prompts: prompts/system.md
---

# Layered Skill Architecture — Reference

This skill demonstrates the **internal** layered organisation recommended for
complex skills (see [`docs/guide/skills.md`](../../../docs/guide/skills.md)
section "Complex Skill Architecture").

It is intentionally simple — three asset-management tools that share a small
service object — so the **structure**, not the business logic, is the focus.

## Layout

```text
example-layered-skill/
├── SKILL.md            ← this file (frontmatter + prose)
├── tools.yaml          ← MCP tool declarations (sibling, per #356)
├── scripts/
│   ├── __init__.py
│   ├── tools/          ← thin adapters (parse params, return envelope)
│   │   ├── __init__.py
│   │   ├── create_asset.py
│   │   ├── publish_asset.py
│   │   └── validate_asset.py
│   ├── services/       ← business logic (orchestration, error handling)
│   │   ├── __init__.py
│   │   └── asset_service.py
│   └── utils/          ← pure helpers (no I/O, no DCC calls, fully unit-testable)
│       ├── __init__.py
│       └── path_utils.py
└── prompts/
    └── system.md       ← optional system prompt sidecar
```

## Layer responsibilities

| Layer | Responsibility | Size guidance |
|-------|----------------|---------------|
| **tools/** | Parse JSON params from stdin, validate, delegate, return envelope. | < 30 lines |
| **services/** | Orchestrate DCC commands. Easily unit-testable in isolation. | Grows with feature |
| **utils/** | Pure functions — path normalisation, primitive helpers. No side effects. | Grows with feature |

## Tools exposed

| Tool | Description |
|------|-------------|
| `example_layered_skill__create_asset` | Create a new asset record |
| `example_layered_skill__publish_asset` | Publish an existing asset |
| `example_layered_skill__validate_asset` | Validate an asset against project rules (read-only) |

## Why a sibling `tools.yaml`

Per [#356](https://github.com/loonghao/dcc-mcp-core/issues/356), tool
declarations live in a sibling YAML referenced from
`metadata.dcc-mcp.tools` so that SKILL.md frontmatter stays
agentskills.io 1.0 compliant.
