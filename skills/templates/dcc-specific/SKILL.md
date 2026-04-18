---
name: my-dcc-skill
description: "A DCC-specific skill template. This skill only appears when the MCP server targets the specified DCC application."
license: MIT
compatibility: Maya 2022+, Python 3.7+
tags: [maya, example]
dcc: maya
version: "1.0.0"
search-hint: "maya, scene, geometry, example"
metadata:
  category: scene
  author: your-name
tools:
  - name: execute
    description: "Execute a command in the target DCC. Replace with your tool's description."
    input_schema:
      type: object
      properties:
        command:
          type: string
          description: "DCC command to execute"
      required: [command]
    read_only: false
    destructive: false
    idempotent: false
    source_file: scripts/execute.py
    next-tools:
      on-success: []
      on-failure: [dcc_diagnostics__screenshot, dcc_diagnostics__audit_log]
---

# my-dcc-skill

A DCC-specific skill that only loads when `dcc_name` matches (e.g. `"maya"`).

## Usage

This skill is automatically discovered when `DCC_MCP_MAYA_SKILL_PATHS` or
`DCC_MCP_SKILL_PATHS` includes the parent directory.

## Notes

- The `dcc: maya` field filters this skill to Maya-only servers.
- The `next-tools` field guides the AI to capture a screenshot on failure.
- Replace `execute.py` with your actual DCC integration logic.
