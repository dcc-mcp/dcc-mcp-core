---
name: houdini-asset-sync
description: Publish and consume immutable USD revisions through the dcc-mcp-core Asset Sync contract.
license: MIT
compatibility: "dcc-mcp-core 0.20.14+ with asset_sync, dcc-mcp-houdini 0.33+, Houdini 20.5+"
metadata:
  dcc-mcp:
    dcc: houdini
    layer: domain
    stage: interchange
    version: "1.0.0"
    tags: [houdini, usd, asset-sync, animation, skeleton, curves, materials]
    search-hint: "sync asset, publish USD revision, cross DCC, Maya interchange, editable animation"
    side-effects:
      creates: true
      modifies: true
      exports: true
      imports: true
    tools: tools.yaml
---

# houdini-asset-sync

Content-addressed Houdini Asset Sync for USD. Operator configuration owns all
filesystem roots; callers provide only bounded identifiers and relative paths.

- `publish_usd_revision` publishes an already-authored USD/USDZ layer without flattening it.
- `read_asset_head` inspects the immutable path-free manifest.
- `reference_usd_revision` materializes a verified revision and creates a Solaris Reference LOP.

Preserving USD composition maximizes fidelity for animation, UsdSkel, BasisCurves,
MaterialX/UsdShade bindings, units, and up-axis metadata. Use the Maya receiver's
`native` mode when direct Maya curve/material editing is preferred over composition fidelity.
