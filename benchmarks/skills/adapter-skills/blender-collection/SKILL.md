---
name: blender-collection
description: "Blender collection management — create, link objects, and organize scene hierarchy"
license: "MIT"
allowed-tools: ["Bash", "Read"]
metadata:
  dcc-mcp:
    dcc: blender
    layer: domain
    stage: scene
    version: "1.0.0"
    tags: [blender, collection, hierarchy, organization]
    search-hint: "collection, group, organize, hierarchy, link object, list collections"
    search-aliases: [create collection, list collections, link object to collection, organize scene, group objects, collection hierarchy]
    intent: "Create and organize Blender collections, link objects, and inspect collection hierarchy for scene organization."
    recall-context:
      app_type: blender
      domain: scene
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
      targets: [collection, scene_node]
    produces: [collection, collection_hierarchy]
    requires: []
    tools: tools.yaml
---

# blender-collection

Blender collection management skill.
