---
name: my-dcc-skill
description: "A DCC-specific skill template. This skill only appears when the MCP server targets the specified DCC application."
license: MIT
compatibility: Maya 2022+, Python 3.7+
metadata:
  category: scene
  author: your-name
  dcc-mcp:
    dcc: maya
    version: "1.0.0"
    tags: "maya, example"
    search-hint: "maya, scene, geometry, example"
    tools: tools.yaml
---

# my-dcc-skill

A DCC-specific skill that only loads when `dcc_name` matches (e.g. `"maya"`).

## Usage

This skill is automatically discovered when `DCC_MCP_MAYA_SKILL_PATHS` or
`DCC_MCP_SKILL_PATHS` includes the parent directory.

## Notes

- The `metadata.dcc-mcp.dcc: maya` field filters this skill to Maya-only servers.
- The `next-tools` field in `tools.yaml` guides the AI to capture a screenshot on failure.
- Replace `execute.py` with your actual DCC integration logic.
