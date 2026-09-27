---
name: zbrush-brush
description: >-
  Domain skill — create and load a reusable wrinkle-crease sculpting brush in
  ZBrush. Use when you need a DamStandard-based ZBP preset with quiet file
  handling and consistent crease, LazyMouse, and ZSub settings.
license: MIT
compatibility: "dcc-mcp-zbrush 0.2+, ZBrush 2026.1+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: zbrush
    layer: domain
    stage: authoring
    version: "1.0.0"
    tags: [zbrush, brush, wrinkle, crease, sculpt]
    search-hint: "create wrinkle brush, load wrinkle brush, crease brush, ZBP"
    tools: tools.yaml
---

# zbrush-brush

Creates a reusable `WrinkleCrease.ZBP` from DamStandard and loads it with the
global Draw settings that ZBrush does not restore from the brush file itself.
Both operations suppress scripted UI feedback and avoid native file pickers.
