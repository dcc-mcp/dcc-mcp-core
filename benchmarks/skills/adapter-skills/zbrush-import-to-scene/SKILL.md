---
name: zbrush-import-to-scene
description: >-
  Pipeline skill — import OBJ asset files into ZBrush as subtools.
  Consumes the dcc-mcp-core asset_import contract (AssetDescriptor) and
  returns an ImportToSceneResult with the imported subtool names. Use when
  you need to bring geometry from disk into the active ZBrush scene.
license: MIT
compatibility: "dcc-mcp-zbrush 0.2+, ZBrush 2026.1+, dcc-mcp-core 0.20.14+"
allowed-tools: Bash Read Write
metadata:
  dcc-mcp:
    dcc: zbrush
    layer: domain
    stage: pipeline
    version: "1.0.0"
    tags: [zbrush, import, obj, mesh, pipeline, asset-import]
    search-hint: "import asset, load mesh, bring in obj, asset import pipeline"
    tools: tools.yaml
---

# zbrush-import-to-scene

Import OBJ asset files into ZBrush using the `dcc-mcp-core` asset import
contract.  The skill validates the `AssetDescriptor`, picks the first
preferred (or first available) variant, and uses the ZBrush SDK
`set_next_filename` + `Tool:Import` pattern to load the geometry. When the
active tool already contains editable geometry, it duplicates the active
subtool first so the existing mesh is preserved. Scripted UI action feedback
and UI updates are suppressed during the import and reliably restored
afterward.

FBX import remains interactive in ZBrush and is rejected before host dispatch.
