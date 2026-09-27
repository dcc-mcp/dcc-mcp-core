---
name: zbrush-interchange
description: >-
  Domain skill — export meshes from ZBrush to interchange formats. Use when you
  need OBJ export from the active subtool. Not for in-scene sculpt edits — use
  zbrush-subtool first to pick the correct subtool.
license: MIT
compatibility: "dcc-mcp-zbrush 0.2+, ZBrush 2026.1+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: zbrush
    layer: domain
    stage: interchange
    version: "1.0.0"
    tags: [zbrush, export, obj, mesh, interchange]
    search-hint: "export obj, save mesh, interchange, send to maya blender"
    tools: tools.yaml
---

# zbrush-interchange

Typed mesh export using the official SDK pattern (`set_next_filename` +
`Tool:Export`) while scripted action drawing and UI updates are suppressed and
reliably restored.
