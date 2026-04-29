---
name: maya-pipeline
description: >-
  Domain skill — Maya asset pipeline orchestration: set up project directory
  structures, export scenes to USD, and coordinate multi-step DCC workflows.
  Use when initialising a Maya project or exporting assets for a downstream
  pipeline. Not for raw geometry editing — use maya-geometry for that. Not for
  low-level USD file inspection — use usd-tools for that.
license: MIT
compatibility: Maya 2022+, Python 3.7+, requires usd-tools and maya-geometry skills
allowed-tools: Bash Read Write
metadata:
  dcc-mcp.dcc: maya
  dcc-mcp.version: "2.0.0"
  dcc-mcp.layer: domain
  dcc-mcp.search-hint: "Maya project setup, export scene USD, asset pipeline, Maya export workflow, DCC pipeline orchestration"
  dcc-mcp.tags: "maya, pipeline, export, project setup, domain"
  dcc-mcp.tools: tools.yaml
depends:
  - maya-geometry
  - usd-tools
---

# Maya Pipeline Skill (Advanced)

Demonstrates the **full advanced skill layout** for `dcc-mcp-core`:

```
maya-pipeline/
├── SKILL.md                  # Frontmatter + instructions
├── scripts/
│   ├── setup_project.py      # Create Maya project structure
│   └── export_usd.py         # Export scene to USD
└── metadata/
    ├── help.md               # Detailed usage docs
    ├── install.md            # Installation / environment setup
    └── uninstall.md          # Cleanup instructions
```

## Tools

### `maya_pipeline__setup_project`
Create a Maya project directory with the standard folder hierarchy:
`scenes/`, `assets/`, `sourceimages/`, `renderData/`, `movies/`.

### `maya_pipeline__export_usd`
Export the current Maya scene (or selection) to Universal Scene Description.
Integrates with `usd-tools` for post-export validation.

## Composability

This skill **depends on** two other skills:

| Dependency | Purpose |
|-----------|---------|
| `maya-geometry` | Reuses geometry creation tools |
| `usd-tools` | USD validation after export |

When loading via `load_skill("maya-pipeline")`, ensure the dependency skills
are loaded first:

```python
catalog.load_skill("maya-geometry")
catalog.load_skill("usd-tools")
catalog.load_skill("maya-pipeline")
```

## Metadata directory

The `metadata/` directory provides structured side-car documentation:

- **help.md** — User-facing reference
- **install.md** — Pre-install steps, environment variables, dependency checks
- **uninstall.md** — Cleanup steps when removing the skill
