---
name: maya-pipeline
description: "Advanced Maya pipeline skill with metadata docs, install hooks, and skill dependencies"
tools: ["Bash", "Read", "Write"]
tags: ["maya", "pipeline", "advanced", "composable"]
dcc: maya
version: "2.0.0"
---

# Maya Pipeline Skill (Advanced)

This skill demonstrates the **advanced skill layout** supported by `dcc-mcp-core`:

```
maya-pipeline/
├── SKILL.md                  # Frontmatter (navigation map) + overview
├── scripts/
│   ├── setup_project.py      # Create Maya project structure
│   └── export_usd.py         # Export scene to USD
└── metadata/
    ├── help.md               # Detailed usage documentation
    ├── install.md            # Installation instructions / hooks
    └── uninstall.md          # Cleanup instructions
```

## Key Features

### 1. Skill Dependencies (`depends`)

This skill declares dependencies on other skills:

```yaml
depends:
  - maya-geometry    # Reuses geometry creation tools
  - usd-tools        # Reuses USD validation
```

When a skill runner resolves `maya-pipeline`, it knows to also load
`maya-geometry` and `usd-tools` first. This enables **composable skill graphs**.

### 2. Metadata Directory

The `metadata/` directory provides structured documentation:

- **help.md** — Detailed user-facing docs (rendered by MCP clients)
- **install.md** — Pre-install steps, environment setup, dependency checks
- **uninstall.md** — Cleanup steps when removing the skill

### 3. Multi-Script Orchestration

Scripts in this skill call into dependent skills' scripts, demonstrating
how skills compose at the execution layer.
