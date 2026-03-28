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

## Scripts

- **create_sphere.py** — Create a polygon sphere with configurable radius and subdivisions
- **batch_rename.py** — Batch rename selected objects with a prefix/suffix pattern

## Example

```python
from dcc_mcp_core import SkillScanner, ActionRegistry

scanner = SkillScanner()
dirs = scanner.scan(extra_paths=["examples/skills"], dcc_name="maya")
# Discovered: maya-geometry
```
