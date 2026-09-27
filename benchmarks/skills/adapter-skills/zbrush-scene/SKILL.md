---
name: zbrush-scene
description: >-
  Domain skill — read-only scene and tool introspection for ZBrush. Use when you
  need active tool path, subtool inventory, or selection index. Not for sculpting,
  export, or subtool mutation — use zbrush-subtool or zbrush-interchange.
license: MIT
compatibility: "dcc-mcp-zbrush 0.2+, ZBrush 2026.1+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read
metadata:
  dcc-mcp:
    dcc: zbrush
    layer: domain
    stage: scene
    version: "1.0.0"
    tags: [zbrush, scene, subtool, tool, inventory]
    search-hint: "scene info, subtools, active tool, tool path, list subtools"
    tools: tools.yaml
---

# zbrush-scene

Read-only scene inventory for the active ZTool. Pair with `zbrush-subtool` when
you need to change the active subtool.
