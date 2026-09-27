---
name: painter-compositing
description: >-
  Inspect and control how a Substance 3D Painter layer composites with the layers
  beneath it - visibility, opacity, blending mode, and mask state. Use when
  stacking layers non-destructively or tuning how one layer combines with another.
  Not for creating or deleting layers, and not for painting strokes.
license: MIT
compatibility: "Substance 3D Painter Python API; dcc-mcp-core 0.20.15+"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: substance3d_painter
    version: "1.0.0"
    layer: domain
    stage: pipeline
    search-hint: "substance painter layer compositing visibility opacity blending mode blend mask enable disable"
    tags: "substance, painter, compositing, layer, opacity, blending, blend-mode, visibility, mask"
    tools: tools.yaml
---

# Painter compositing

Every tool addresses exactly one layer by the UID returned from
`painter_project__list_layer_stack`, and every mutation is confirmed by reading
the node back from the selected Painter stack before reporting success. A tool
reports failure rather than claiming an effect it could not verify.

Painter's compositing primitives are exposed per layer node. This skill resolves
them against the running host instead of hard-coding enum constants, because the
exact set of blending modes and the accessor names vary between Painter
releases:

| Capability | Host accessor | Missing on host |
| --- | --- | --- |
| Visibility | `is_visible()` / `set_visible(bool)` | `layer_visibility_unsupported` |
| Opacity | `get_opacity()` / `set_opacity(float)` | `layer_opacity_unsupported` |
| Blending mode | `get_blending_mode()` / `set_blending_mode(member)` | `layer_blending_mode_unsupported` |
| Mask state | `has_mask()` / `is_mask_enabled()` / `get_mask_background()` | reported as `null` fields |

Use `inspect_layer_compositing` first to discover which of these the running
Painter build actually supports, and to read the valid blending-mode names when
`set_layer_blending_mode` rejects a value. Opacity is expressed on the unit
interval `0.0` to `1.0`, matching Painter's own slider range.
