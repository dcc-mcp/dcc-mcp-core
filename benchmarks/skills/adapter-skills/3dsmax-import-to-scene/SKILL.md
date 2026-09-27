---
name: 3dsmax-import-to-scene
description: >-
  Agent-facing skill for importing asset descriptors into Autodesk 3ds Max.
  Use this skill when a contract payload from dcc_mcp_core.asset_import needs
  to be turned into scene nodes.
license: MIT
metadata:
  dcc-mcp:
    dcc: 3dsmax
    layer: operator
    stage: authoring
    tools: tools.yaml
    tags: "3dsmax, asset-import, fbx, obj, placement"
---

# 3ds Max Import To Scene

Use this skill to import an `ImportToSceneRequest` payload into 3ds Max.
Prefer FBX variants when available. The bundled runtime action resolves the
best variant, routes through the existing FBX or generic geometry import
actions, and returns a serialized `ImportToSceneResult`.
