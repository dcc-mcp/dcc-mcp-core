---
name: marmoset-diagnostics
description: >-
  Domain skill - inspect Toolbag runtime, GPU, renderer, preferences, materials,
  and missing texture paths; release unused resources; and control tooltip
  display through bounded typed operations. Use for TA diagnostics and scene
  health checks. Not for arbitrary Python execution.
license: MIT
compatibility: "Marmoset Toolbag 4.03+ or 5.x; Python 3.9+; dcc-mcp-core 0.20.14+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: marmoset
    layer: domain
    version: "0.1.2"  # x-release-please-version
    stage: diagnostics
    search-hint: "Marmoset Toolbag diagnostics GPU renderer preferences missing textures assets VRAM tooltips TA debug"
    tags: "marmoset,toolbag,diagnostics,debugging,technical-art"
    tools: tools.yaml
---

# Marmoset Toolbag Diagnostics

Inspect before changing preferences or releasing resources. Missing texture
reports contain local paths and are intended for the current workstation only.
`inspect_runtime` reports the tooltip database directory and file count.
`set_display_tooltips` changes a persistent Toolbag preference, but does not
repair or silence a missing-database startup error; report that host packaging
defect separately.

If a Core job remains pending while Toolbag is unfocused, activate that exact
instance through native DCC UI Control, then poll the existing job. Do not retry
the mutation and do not launch another Toolbag process.
