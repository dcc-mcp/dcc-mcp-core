---
name: photoshop-layers
description: Adobe Photoshop layer operations — create, delete, reorder, duplicate, rename, set opacity/visibility, fill,
  blend modes
license: MIT
allowed-tools:
- Bash
- Read
metadata:
  dcc-mcp:
    dcc: photoshop
    version: 0.1.0
    layer: domain
    tags:
    - photoshop
    - layers
    - opacity
    - visibility
    - blend
    - adobe
    search-hint: layer create delete rename duplicate opacity blend mode fill group
    tools: tools.yaml
---
# photoshop-layers

Layer management skill for Adobe Photoshop. Provides complete CRUD operations
on document layers plus visual property changes (opacity, blend mode, fill).

## Tools

- `create_layer` — Create pixel / group layer
- `delete_layer` — Delete a layer by name
- `duplicate_layer` — Duplicate a layer
- `rename_layer` — Rename a layer
- `set_layer_opacity` — Change opacity 0-100
- `set_layer_visibility` — Show / hide
- `set_layer_blend_mode` — Change blend mode
- `fill_layer` — Fill with solid color
