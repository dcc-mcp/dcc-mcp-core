---
name: maya-geometry
description: "Maya geometry creation and modification tools"
tools: ["Bash", "Read", "Write"]
tags: ["maya", "geometry", "creation"]
dcc: maya
version: "1.0.0"
---

# Maya Geometry Skill

Provides tools for creating and modifying geometry in Autodesk Maya.
This skill demonstrates the canonical DCC skill pattern: scripts that call Maya Python API,
wrapped as MCP tools via `dcc-mcp-core`.

## Scripts

- **create_sphere.py** — Create a polygon sphere with configurable radius and subdivisions
- **batch_rename.py** — Batch rename selected objects with a prefix/suffix pattern

## Action Names (auto-derived)

When loaded, this skill registers these MCP actions:
- `maya_geometry__create_sphere` — from `scripts/create_sphere.py`
- `maya_geometry__batch_rename` — from `scripts/batch_rename.py`

Naming rule: `{skill_name}__{script_stem}` (hyphens → underscores, `__` separator)

## Usage Example

```python
from dcc_mcp_core import scan_and_load, ActionRegistry
from pathlib import Path

# Discover and load this skill
skills, skipped = scan_and_load(
    extra_paths=["examples/skills"],
    dcc_name="maya",
)
# skills[0].name == "maya-geometry"
# skills[0].scripts == [".../scripts/batch_rename.py", ".../scripts/create_sphere.py"]

# Register in an ActionRegistry
reg = ActionRegistry()
for skill in skills:
    for script_path in skill.scripts:
        stem = Path(script_path).stem
        action_name = f"{skill.name.replace('-', '_')}__{stem}"
        reg.register(name=action_name, description=skill.description, dcc=skill.dcc)

print(reg.list_actions_for_dcc("maya"))
# ["maya_geometry__batch_rename", "maya_geometry__create_sphere"]
```

## Prerequisites

- Autodesk Maya 2022+ with Python 3
- Maya Python API (`maya.cmds`) available in the execution environment
