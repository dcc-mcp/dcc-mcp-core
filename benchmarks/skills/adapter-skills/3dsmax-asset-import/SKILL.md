---
name: 3dsmax-asset-import
description: >-
  Domain skill - import 3D assets (FBX, OBJ, 3DS, USD) into the current 3ds Max
  scene via the AssetDescriptor or MAXtoA USD procedural contracts. Use when
  the user wants external geometry or a renderable USD stage in the scene.
license: MIT
compatibility: "dcc-mcp-core 0.17+, 3ds Max 2024+"
metadata:
  dcc-mcp:
    dcc: 3dsmax
    version: "1.0.0"
    layer: domain
    stage: authoring
    search-hint: "3ds Max asset import fbx obj 3ds usd arnold procedural geometry scene"
    tags: "3dsmax, asset, import, fbx, obj, usd, arnold, geometry"
    tools: tools.yaml
    intent: "Import geometry or create a MAXtoA procedural for a USD stage in 3ds Max."
    search_aliases: ["asset import", "import", "arnold usd", "usd procedural"]
    recall_context:
      app_type: "3dsmax"
      domain: "asset_import"
      workflow_stage: "authoring"
      task_category: "mutate"
    preconditions:
      - type: software
        name: "3ds Max"
        version: ">=2024"
    side_effects:
      creates: true
      modifies: false
      deletes: false
      exports: false
      imports: true
      file_output: false
      render: false
      targets: ["scene_node", "file"]
    produces: ["scene_node", "asset_import"]
---

# 3ds Max Asset Import Skill

Import 3D assets into the current scene using dcc-mcp-core's `AssetDescriptor`
contract. Supported formats: FBX, OBJ, 3DS, USD, USDA, USDC, and USDZ.

Each import operation snapshots the node set before and after the import,
reports what was created, and renames imported nodes with a consistent prefix.

The `import_to_scene` tool accepts either a dict payload (matching the
AssetDescriptor schema) or a descriptor object and returns the created node
identities with a combined bounding box.

The `create_arnold_usd_procedural` tool creates a typed MAXtoA procedural for a
USD stage. It preserves the source path, optional USD prim path, time sample,
and authored up-axis so animated USD content can be rendered without arbitrary
Python execution.
