---
name: marketplace-create-extension
description: >-
  Infrastructure skill — scaffold a new marketplace extension package
  (SKILL.md + tools.yaml + scripts/) with MIT-0 licensing. Use when
  creating a publishable marketplace entry for any DCC host. Not for
  editing existing extensions or driving live DCC scenes — use domain
  skills for that.
license: MIT-0
compatibility: "dcc-mcp-core 0.17+, Python 3.7+"
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: python
    version: "0.18.9"  # x-release-please-version
    layer: infrastructure
    search-hint: >-
      create marketplace extension, scaffold extension package, new skill
      package, SKILL.md generator, marketplace entry, extension authoring
    tags: "marketplace, extension, scaffolding, authoring, infrastructure"
    tools: tools.yaml
    skill-reference-docs:
      - "references/*.md"
  openclaw:
    homepage: https://github.com/dcc-mcp/dcc-mcp-core/blob/main/skills/marketplace-create-extension/SKILL.md
---

# Marketplace Create Extension

Scaffold a new marketplace extension package with the standard dcc-mcp
skill layout and MIT-0 licensing. Follows the same patterns as
`dcc-mcp-skills-creator`.

## Tools

### `marketplace_create_extension__create`
Scaffold a new extension directory containing SKILL.md (MIT-0), tools.yaml
(with full annotations), and a scripts/ directory with a placeholder action
script that uses `dcc_mcp_core.skills_helper`.

## Prerequisites

- dcc-mcp-core installed
- Write access to the target output directory

## Generated Layout

```
<name>/
├── SKILL.md       # MIT-0 frontmatter + body
├── tools.yaml     # Tool declarations with schemas, annotations, execution metadata
└── scripts/
    └── <action>.py  # Placeholder using skills_helper pattern
```

## Quick Start

```python
from dcc_mcp_core.skills_helper import call_tool

# Via MCP tool call:
# marketplace_create_extension__create(
#     name="my-maya-tools",
#     description="Custom Maya modeling utilities",
#     dcc_targets=["maya"],
#     install_type="git",
#     author="Your Name",
# )
```

## Authoring Workflow

1. Scaffold the extension with `marketplace_create_extension__create`.
2. Implement the placeholder action script.
3. Validate with `dcc_mcp_skills_creator__validate_skill_dir`.
4. Publish to a marketplace catalog with `marketplace_publish_extension__publish`.

Read [MARKETPLACE_EXTENSION_GUIDE.md](references/MARKETPLACE_EXTENSION_GUIDE.md)
for the full marketplace extension authoring contract.
