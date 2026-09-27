---
name: aftereffects-advanced
description: Reach the complete official After Effects object model through structured DOM references, with explicit raw ExtendScript fallback for API gaps.
license: MIT
compatibility: "After Effects CEP/ExtendScript; adobepy officialDom"
allowed-tools: Python
metadata:
  dcc-mcp:
    dcc: aftereffects
    version: "0.1.0"
    layer: advanced
    stage: scene
    search-hint: "after effects official dom object model extendscript advanced api"
    tags: "adobe,aftereffects,dom,extendscript"
    tools: tools.yaml
---

# After Effects Advanced API

Prefer `official_dom`: it performs structured root/get/set/call/construct/keys/
snapshot/release operations without source evaluation. DOM references returned
as `{"$ref": "...", "$type": "..."}` are session-scoped and must not be
reused after release or bridge restart.

Use raw ExtendScript only when the typed facade and official DOM cannot express
the operation. Treat raw source as destructive and inspect it before execution.
