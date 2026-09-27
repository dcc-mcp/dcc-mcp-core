---
name: aftereffects-layers
description: Inspect and edit After Effects compositions, layers, transforms, text, effects, masks, and keyframes through typed adobepy facades.
license: MIT
compatibility: "After Effects CEP/ExtendScript; dcc-mcp-core >=0.20.14,<1.0.0"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: aftereffects
    version: "0.1.0"
    layer: domain
    stage: scene
    search-hint: "after effects composition layers text transform keyframes effects masks"
    tags: "adobe,aftereffects,composition,layers,animation"
    tools: tools.yaml
---

# After Effects Layers

Use typed layer tools for ordinary work. Resolve a composition and layer by id,
index, or exact name. Inspect before mutation, then verify the returned layer.
