---
name: maya-pipeline
description: "Advanced Maya pipeline skill — set up project structures, export scenes to USD, and orchestrate multi-step DCC workflows. Use when initialising a Maya project or exporting assets for a pipeline."
license: MIT
compatibility: Maya 2022+, Python 3.7+, requires usd-tools and maya-geometry skills
allowed-tools: Bash Read Write
metadata:
  category: pipeline
  author: dcc-mcp-core
tags: [maya, pipeline, advanced, composable]
dcc: maya
version: "2.0.0"
search-hint: "maya, pipeline, USD, export, project setup, scene, asset, DCC workflow"
depends:
  - maya-geometry
  - usd-tools
tools:
  - name: setup_project
    description: Create a Maya project directory structure with standard folders
    input_schema:
      type: object
      required: [project_path]
      properties:
        project_path:
          type: string
          description: Absolute path where the Maya project will be created
        project_name:
          type: string
          description: Name of the Maya project
    read_only: false
    destructive: false
    idempotent: true
    source_file: scripts/setup_project.py

  - name: export_usd
    description: Export the current Maya scene to a USD file
    input_schema:
      type: object
      required: [output_path]
      properties:
        output_path:
          type: string
          description: Destination USD file path (.usd, .usda, .usdz)
        selection_only:
          type: boolean
          description: Export only selected objects
          default: false
        animation:
          type: boolean
          description: Include animation data
          default: true
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/export_usd.py
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
