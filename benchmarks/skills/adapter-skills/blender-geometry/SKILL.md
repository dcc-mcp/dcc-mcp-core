---
name: blender-geometry
description: "Blender geometry creation and export tools for real bpy workflows"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: authoring
    version: "1.0.0"
    tags: [blender, geometry, export, mesh]
    search-hint: "create sphere, save blend, export fbx, export obj, file exists"
    search-aliases: [primitive, sphere, save as, save blend file, file check, geometric primitive, basic mesh]
    intent: "Create basic geometry primitives and save/export Blender files with file existence checks."
    recall-context:
      app_type: blender
      domain: authoring
      workflow_stage: authoring
      task_category: mutate
    preconditions:
      - type: software
        name: blender
        version: ">=4.0"
      - type: scene_state
        predicate: has_open_scene
    side-effects:
      creates: true
      modifies: true
      file_output: true
      targets: [scene_node, file:blend, file:fbx, file:obj]
    produces: [mesh_object, file:blend, file:fbx, file:obj]
    requires: []
    tools: tools.yaml
---

# blender-geometry

Geometry tools for Blender main-thread execution and export validation.
