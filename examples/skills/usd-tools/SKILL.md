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

## Pipeline Integration

```python
# In a DCC MCP server, skills are auto-registered
scanner = SkillScanner()
scanner.scan(extra_paths=["/studio/pipeline/skills"], dcc_name="maya")
# -> discovers usd-tools, registers inspect & validate as MCP tools
```
