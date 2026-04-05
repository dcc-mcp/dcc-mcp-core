---
name: usd-tools
description: "OpenUSD scene inspection and validation tools"
tools: ["Bash", "Read"]
tags: ["usd", "openusd", "pipeline", "scene", "validation"]
dcc: python
version: "1.0.0"
metadata:
  openclaw:
    requires:
      bins:
        - usdcat
        - usdchecker
---

# OpenUSD Tools Skill

Integrates [OpenUSD](https://openusd.org/) command-line tools for scene
inspection, validation, and conversion. OpenUSD is the industry standard for
3D scene interchange used across Maya, Houdini, Blender, Unreal Engine, and more.

This skill is a prime example of `dcc-mcp-core`'s mission: bridging DCC
applications through MCP.

## Scripts

- **inspect.py** — Inspect USD stage structure (prims, layers, composition arcs)
- **validate.py** — Validate USD files using usdchecker compliance rules

## Action Names (auto-derived)

- `usd_tools__inspect` — from `scripts/inspect.py`
- `usd_tools__validate` — from `scripts/validate.py`

## Integration with dcc-mcp-core USD API

This skill works alongside the built-in `UsdStage` API in `dcc-mcp-core`:

```python
from dcc_mcp_core import UsdStage, SdfPath, VtValue, scene_info_json_to_stage

# Build a USD stage programmatically
stage = UsdStage("my_scene")
stage.define_prim("/World", "Xform")
prim = stage.define_prim("/World/Cube", "Mesh")
prim.set_attribute("extent", VtValue.from_vec3f(1.0, 1.0, 1.0))

# Export to USDA (ASCII format) — then pass to usd-tools scripts
usda_content = stage.export_usda()

# Convert a DCC SceneInfo (from Maya/Blender adapter) to UsdStage
stage2 = scene_info_json_to_stage(scene_info_json_str, dcc_type="maya")
```

## Pipeline Integration

```python
# In a DCC MCP server, skills are auto-registered
from dcc_mcp_core import scan_and_load

skills, _ = scan_and_load(
    extra_paths=["/studio/pipeline/skills"],
    dcc_name="python",  # usd-tools targets generic python DCC
)
# Registers usd_tools__inspect and usd_tools__validate as MCP tools
```

## Prerequisites

- Python 3.7+
- OpenUSD tools installed: `usdcat`, `usdchecker` (from `pip install usd-core` or USD distribution)
